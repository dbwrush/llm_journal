use crate::config::Config;
use crate::cycle_date::CycleDate;
use crate::journal::{JournalManager, PromptType};
use crate::llm_worker::LlmManager;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use chrono::{Local, NaiveTime};

/// Background service that generates daily prompts at a scheduled time
pub struct PromptGenerator {
    journal_manager: Arc<JournalManager>,
    llm_manager: Arc<LlmManager>,
    config: Arc<Config>,
    is_running: Arc<tokio::sync::Mutex<bool>>,
}

impl PromptGenerator {
    pub fn new(
        journal_manager: Arc<JournalManager>,
        llm_manager: Arc<LlmManager>,
        config: Arc<Config>,
    ) -> Self {
        Self {
            journal_manager,
            llm_manager,
            config,
            is_running: Arc::new(tokio::sync::Mutex::new(false)),
        }
    }

    /// Start the background prompt generation service
    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut is_running = self.is_running.lock().await;
        if *is_running {
            tracing::warn!("⚠️  Prompt generator is already running");
            return Ok(());
        }
        *is_running = true;
        drop(is_running);

        tracing::info!("🚀 Starting prompt generator service");
        tracing::info!("   📅 Daily prompt generation scheduled for: {}", self.config.journal.prompt_generation_time);
        
        // Clone references for the background task
        let journal_manager = Arc::clone(&self.journal_manager);
        let llm_manager = Arc::clone(&self.llm_manager);
        let config = Arc::clone(&self.config);
        let is_running = Arc::clone(&self.is_running);

        // Spawn background task
        tokio::spawn(async move {
            // Check if we need to generate prompts immediately on startup
            if let Err(e) = Self::check_and_generate_startup_prompts(
                Arc::clone(&journal_manager),
                Arc::clone(&llm_manager),
                Arc::clone(&config),
            ).await {
                tracing::error!("❌ Failed to check/generate startup prompts: {}", e);
            }

            loop {
                // Check if we should still be running
                {
                    let running = is_running.lock().await;
                    if !*running {
                        tracing::info!("🛑 Prompt generator service stopped");
                        break;
                    }
                }

                // Calculate time until next prompt generation
                if let Ok(sleep_duration) = Self::calculate_sleep_until_prompt_time(&config.journal.prompt_generation_time) {
                    tracing::info!("⏰ Next prompt generation in {:.1} hours", sleep_duration.as_secs_f64() / 3600.0);
                    
                    // Sleep until prompt generation time
                    sleep(sleep_duration).await;
                    
                    // Generate prompts for today
                    if let Err(e) = Self::generate_daily_prompts(
                        Arc::clone(&journal_manager),
                        Arc::clone(&llm_manager),
                        Arc::clone(&config),
                    ).await {
                        tracing::error!("❌ Failed to generate daily prompts: {}", e);
                    }
                    
                    // Sleep for a minute to avoid immediate re-triggering
                    sleep(Duration::from_secs(60)).await;
                } else {
                    tracing::error!("❌ Invalid prompt generation time format, sleeping for 1 hour");
                    sleep(Duration::from_secs(3600)).await;
                }
            }
        });

        Ok(())
    }

    /// Stop the background prompt generation service
    pub async fn stop(&self) {
        let mut is_running = self.is_running.lock().await;
        *is_running = false;
        tracing::info!("🛑 Prompt generator service stopping...");
    }

    /// Calculate duration to sleep until the specified time today (or tomorrow if time has passed)
    fn calculate_sleep_until_prompt_time(time_str: &str) -> Result<Duration, String> {
        // Parse the time string (e.g., "06:00")
        let target_time = NaiveTime::parse_from_str(time_str, "%H:%M")
            .map_err(|e| format!("Invalid time format: {}", e))?;
        
        let now = Local::now();
        let today = now.date_naive();
        
        // Create target datetime for today
        let mut target_datetime = today.and_time(target_time).and_local_timezone(Local).single()
            .ok_or("Failed to create target datetime")?;
        
        // If the time has already passed today, schedule for tomorrow
        if target_datetime <= now {
            target_datetime = target_datetime + chrono::Duration::days(1);
        }
        
        let duration_until_target = (target_datetime - now).to_std()
            .map_err(|e| format!("Duration conversion failed: {}", e))?;
        Ok(duration_until_target)
    }

    /// Generate prompts for today
    async fn generate_daily_prompts(
        journal_manager: Arc<JournalManager>,
        llm_manager: Arc<LlmManager>,
        config: Arc<Config>,
    ) -> Result<(), String> {
        let today = CycleDate::today();
        tracing::info!("🌅 Generating daily prompts for {}", today);

        // Check if prompts already exist for today
        let existing_prompts = Self::count_existing_prompts(&journal_manager, &today).await;
        if existing_prompts >= config.journal.max_prompts_per_day {
            tracing::info!("✅ Prompts already exist for today ({}/{})", existing_prompts, config.journal.max_prompts_per_day);
            return Ok(());
        }

        // Load the LLM model
        tracing::info!("🔄 Loading LLM model for prompt generation...");
        llm_manager.prepare_for_processing().await.map_err(|e| e.to_string())?;
        let llm_worker = llm_manager.get_worker();

        // Determine prompt type based on today's position in the cycle
        let prompt_type = if today.is_first_day_of_year() {
            PromptType::YearlyReflection
        } else if today.is_first_day_of_month() {
            PromptType::MonthlyReflection
        } else if today.is_first_day_of_week() {
            PromptType::WeeklyReflection
        } else {
            PromptType::Daily
        };

        // Get context for prompt generation
        let context = journal_manager.get_context_for_prompt(&today).await.map_err(|e| e.to_string())?;

        // Generate the missing prompts
        for prompt_number in (existing_prompts + 1)..=config.journal.max_prompts_per_day {
            tracing::info!("📝 Generating prompt {} for {}", prompt_number, today);
            
            let prompt = llm_worker.generate_prompt(
                &today,
                &context,
                prompt_number,
                prompt_type.clone(),
            ).await.map_err(|e| e.to_string())?;
            
            journal_manager.save_prompt(&prompt).await.map_err(|e| e.to_string())?;
            
            tracing::info!("✅ Prompt {} saved for {}", prompt_number, today);
        }

        tracing::info!("🎉 Daily prompt generation completed for {}", today);
        Ok(())
    }

    /// Count how many prompts already exist for a given date
    async fn count_existing_prompts(journal_manager: &JournalManager, cycle_date: &CycleDate) -> u8 {
        let mut count = 0;
        for i in 1..=3 {  // Max 3 prompts
            if journal_manager.load_prompt(cycle_date, i).await.is_ok() {
                if journal_manager.load_prompt(cycle_date, i).await.unwrap().is_some() {
                    count += 1;
                }
            }
        }
        count
    }

    /// Generate a specific prompt on-demand (for when user navigates past existing prompts)
    pub async fn generate_prompt_on_demand(
        &self,
        cycle_date: &CycleDate,
        prompt_number: u8,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if prompt_number > self.config.journal.max_prompts_per_day {
            return Err(format!("Cannot generate prompt {}, max is {}", prompt_number, self.config.journal.max_prompts_per_day).into());
        }

        // Check if prompt already exists
        if let Ok(Some(_)) = self.journal_manager.load_prompt(cycle_date, prompt_number).await {
            tracing::info!("✅ Prompt {} already exists for {}", prompt_number, cycle_date);
            return Ok(());
        }

        tracing::info!("🔄 Generating on-demand prompt {} for {}", prompt_number, cycle_date);

        // Load the LLM model
        self.llm_manager.prepare_for_processing().await?;
        let llm_worker = self.llm_manager.get_worker();

        // Determine prompt type
        let prompt_type = if cycle_date.is_first_day_of_year() {
            PromptType::YearlyReflection
        } else if cycle_date.is_first_day_of_month() {
            PromptType::MonthlyReflection
        } else if cycle_date.is_first_day_of_week() {
            PromptType::WeeklyReflection
        } else {
            PromptType::Daily
        };

        // Get context for prompt generation
        let context = self.journal_manager.get_context_for_prompt(cycle_date).await?;

        // Generate the prompt
        let prompt = llm_worker.generate_prompt(
            cycle_date,
            &context,
            prompt_number,
            prompt_type,
        ).await?;
        
        self.journal_manager.save_prompt(&prompt).await?;
        
        tracing::info!("✅ On-demand prompt {} generated and saved for {}", prompt_number, cycle_date);
        Ok(())
    }

    /// Check if we're past the prompt generation time for today and generate prompts if needed
    async fn check_and_generate_startup_prompts(
        journal_manager: Arc<JournalManager>,
        llm_manager: Arc<LlmManager>,
        config: Arc<Config>,
    ) -> Result<(), String> {
        let today = CycleDate::today();
        let now = Local::now();
        
        // Parse the configured prompt generation time
        let target_time = NaiveTime::parse_from_str(&config.journal.prompt_generation_time, "%H:%M")
            .map_err(|e| format!("Invalid time format: {}", e))?;
        
        // Check if current time is past the prompt generation time for today
        let current_time = now.time();
        if current_time >= target_time {
            tracing::info!("🔍 Startup check: Current time ({}) is past prompt generation time ({})", 
                current_time.format("%H:%M"), target_time.format("%H:%M"));
            
            // Check if we already have prompts for today
            let existing_prompts = Self::count_existing_prompts(&journal_manager, &today).await;
            if existing_prompts == 0 {
                tracing::info!("🚀 No prompts found for today, generating them now...");
                Self::generate_daily_prompts(journal_manager, llm_manager, config).await?;
            } else {
                tracing::info!("✅ Found {} existing prompts for today, no need to generate", existing_prompts);
            }
        } else {
            tracing::info!("⏰ Startup check: Current time ({}) is before prompt generation time ({}), will wait", 
                current_time.format("%H:%M"), target_time.format("%H:%M"));
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_sleep_duration() {
        // Test with a time format
        let result = PromptGenerator::calculate_sleep_until_prompt_time("06:00");
        assert!(result.is_ok());
        
        // Test with invalid format
        let result = PromptGenerator::calculate_sleep_until_prompt_time("invalid");
        assert!(result.is_err());
    }
}
