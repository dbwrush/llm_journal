use crate::config::Config;
use crate::cycle_date::CycleDate;
use crate::journal::{JournalManager, PromptType};
use crate::llm_worker::LlmManager;
use crate::personalization::PersonalizationConfig;
use crate::prompts::PromptsConfig;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use chrono::{Local, NaiveTime};

/// Background service that generates daily prompts at a scheduled time
pub struct PromptGenerator {
    journal_manager: Arc<JournalManager>,
    llm_manager: Arc<LlmManager>,
    config: Arc<Config>,
    personalization_config: Arc<PersonalizationConfig>,
    is_running: Arc<tokio::sync::Mutex<bool>>,
}

impl PromptGenerator {
    pub fn new(
        journal_manager: Arc<JournalManager>,
        llm_manager: Arc<LlmManager>,
        config: Arc<Config>,
        personalization_config: Arc<PersonalizationConfig>,
    ) -> Self {
        Self {
            journal_manager,
            llm_manager,
            config,
            personalization_config,
            is_running: Arc::new(tokio::sync::Mutex::new(false)),
        }
    }

    /// Start the background prompt generation service
    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut is_running = self.is_running.lock().await;
        if *is_running {
            tracing::warn!("Prompt generator is already running");
            return Ok(());
        }
        *is_running = true;
        drop(is_running);

        tracing::info!("Starting prompt generator service");
        tracing::info!("   Unified daily processing (summaries, status, prompts) scheduled for: {}", self.config.journal.prompt_generation_time);
        
        // Clone references for the background task
        let journal_manager = Arc::clone(&self.journal_manager);
        let llm_manager = Arc::clone(&self.llm_manager);
        let config = Arc::clone(&self.config);
        let personalization_config = Arc::clone(&self.personalization_config);
        let is_running = Arc::clone(&self.is_running);

        // Spawn background task
        tokio::spawn(async move {
            // Check if we need to generate prompts immediately on startup
            if let Err(e) = Self::check_and_generate_startup_prompts(
                Arc::clone(&journal_manager),
                Arc::clone(&llm_manager),
                Arc::clone(&config),
                Arc::clone(&personalization_config),
            ).await {
                tracing::error!("Failed to check/generate startup prompts: {}", e);
            }

            loop {
                // Check if we should still be running
                {
                    let running = is_running.lock().await;
                    if !*running {
                        tracing::info!("Prompt generator service stopped");
                        break;
                    }
                }

                // Calculate time until next prompt generation
                if let Ok(sleep_duration) = Self::calculate_sleep_until_prompt_time(&config.journal.prompt_generation_time) {
                    tracing::info!("Next prompt generation in {:.1} hours", sleep_duration.as_secs_f64() / 3600.0);
                    
                    // Sleep until prompt generation time
                    sleep(sleep_duration).await;
                    
                    // Generate prompts for today
                    if let Err(e) = Self::generate_daily_prompts(
                        Arc::clone(&journal_manager),
                        Arc::clone(&llm_manager),
                        Arc::clone(&config),
                        Arc::clone(&personalization_config),
                    ).await {
                        tracing::error!("Failed to generate daily processing (summaries, status, prompts): {}", e);
                    }
                    
                    // Sleep for a minute to avoid immediate re-triggering
                    sleep(Duration::from_secs(60)).await;
                } else {
                    tracing::error!("Invalid prompt generation time format, sleeping for 1 hour");
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
        tracing::info!("Prompt generator service stopping...");
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

    /// Unified prompt generation function with optional summary/status checks
    /// - skip_checks: true to skip summary/status generation (for 2nd and 3rd prompts in daily batch)
    async fn generate_prompts_unified(
        journal_manager: Arc<JournalManager>,
        llm_manager: Arc<LlmManager>,
        config: Arc<Config>,
        personalization_config: Arc<PersonalizationConfig>,
        cycle_date: &CycleDate,
        skip_checks: bool,
        max_prompts_override: Option<u8>,
    ) -> Result<(), String> {
        tracing::info!("Generating prompts for {} (skip_checks: {})", cycle_date, skip_checks);

        // Check if prompts already exist
        let max_prompts = max_prompts_override.unwrap_or(config.journal.max_prompts_per_day);
        let existing_prompts = Self::count_existing_prompts(&journal_manager, cycle_date).await;
        if existing_prompts >= max_prompts {
            tracing::info!("Prompts already exist for {} ({}/{})", cycle_date, existing_prompts, max_prompts);
            return Ok(());
        }

        // Load the LLM model
        tracing::debug!("Loading LLM model for prompt generation...");
        llm_manager.prepare_for_processing().await.map_err(|e| e.to_string())?;
        let llm_worker = llm_manager.get_worker();

        // Determine prompt type based on date's position in the cycle
        let prompt_type = if cycle_date.is_first_day_of_year() {
            PromptType::YearlyReflection
        } else if cycle_date.is_first_day_of_month() {
            PromptType::MonthlyReflection
        } else if cycle_date.is_first_day_of_week() {
            PromptType::WeeklyReflection
        } else {
            PromptType::Daily
        };

        // Generate the missing prompts, with optimized checks
        for prompt_number in (existing_prompts + 1)..=max_prompts {
            tracing::info!("Generating prompt {} for {}", prompt_number, cycle_date);
            
            // Only run summary/status checks for the first prompt, unless explicitly requested
            let should_skip_checks = skip_checks || (prompt_number > 1);
            
            if !should_skip_checks {
                tracing::debug!("Checking for entries that need summaries and status files...");
                if let Err(e) = Self::generate_missing_summaries(&journal_manager, &llm_worker, &personalization_config).await {
                    tracing::warn!("Failed to generate some summaries/status files: {}", e);
                    // Continue anyway - prompts can still be generated without perfect context
                }
            } else {
                tracing::debug!("Skipping summary/status checks for prompt {}", prompt_number);
            }

            // Get context for prompt generation (will use existing summaries if available)
            let context = journal_manager.get_context_for_prompt(cycle_date).await.map_err(|e| e.to_string())?;
            
            let prompt = llm_worker.generate_prompt(
                cycle_date,
                &context,
                prompt_number,
                prompt_type.clone(),
                &personalization_config,
            ).await.map_err(|e| e.to_string())?;
            
            journal_manager.save_prompt(&prompt).await.map_err(|e| e.to_string())?;
            
            tracing::info!("Prompt {} saved for {}", prompt_number, cycle_date);
        }

        tracing::info!("Prompt generation completed for {}", cycle_date);
        Ok(())
    }

    /// Generate prompts for today (unified daily processing)
    /// This function handles all daily processing at the scheduled time:
    /// 1. Generates missing summaries and status files for old entries
    /// 2. Generates today's prompts with proper context
    async fn generate_daily_prompts(
        journal_manager: Arc<JournalManager>,
        llm_manager: Arc<LlmManager>,
        config: Arc<Config>,
        personalization_config: Arc<PersonalizationConfig>,
    ) -> Result<(), String> {
        let today = CycleDate::today();
        Self::generate_prompts_unified(
            journal_manager,
            llm_manager,
            config,
            personalization_config,
            &today,
            false, // Don't skip checks for daily generation
            None,  // Use default max_prompts_per_day
        ).await
    }

    /// Public function for external callers (like journal processor)
    pub async fn generate_prompts_for_date(
        journal_manager: Arc<JournalManager>,
        llm_manager: Arc<LlmManager>,
        config: Arc<Config>,
        personalization_config: Arc<PersonalizationConfig>,
        cycle_date: &CycleDate,
        skip_checks: bool,
        max_prompts_override: Option<u8>,
    ) -> Result<(), String> {
        Self::generate_prompts_unified(
            journal_manager,
            llm_manager,
            config,
            personalization_config,
            cycle_date,
            skip_checks,
            max_prompts_override,
        ).await
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
        _prompts_config: &PromptsConfig,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if prompt_number > self.config.journal.max_prompts_per_day {
            return Err(format!("Cannot generate prompt {}, max is {}", prompt_number, self.config.journal.max_prompts_per_day).into());
        }

        // Check if prompt already exists
        if let Ok(Some(_)) = self.journal_manager.load_prompt(cycle_date, prompt_number).await {
            tracing::info!("Prompt {} already exists for {}", prompt_number, cycle_date);
            return Ok(());
        }

        tracing::debug!("Generating on-demand prompt {} for {}", prompt_number, cycle_date);

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
            &self.personalization_config,
        ).await?;
        
        self.journal_manager.save_prompt(&prompt).await?;
        
        tracing::info!("On-demand prompt {} generated and saved for {}", prompt_number, cycle_date);
        Ok(())
    }

    /// Queue prompt generation asynchronously without waiting for completion
    /// This is ideal for triggering prompt generation from web handlers without blocking the response
    pub fn queue_prompt_generation(&self, cycle_date: CycleDate, prompt_number: u8, _prompts_config: &PromptsConfig) {
        let journal_manager = Arc::clone(&self.journal_manager);
        let llm_manager = Arc::clone(&self.llm_manager);
        let personalization_config = Arc::clone(&self.personalization_config);
        
        tracing::debug!("Queuing prompt {} generation for {} (async)", prompt_number, cycle_date);
        
        // Spawn a background task to handle the generation
        tokio::spawn(async move {
            // Remove the max_prompts_per_day limitation for unlimited prompts
            if let Ok(Some(_)) = journal_manager.load_prompt(&cycle_date, prompt_number).await {
                tracing::debug!("Prompt {} already exists for {}, skipping", prompt_number, cycle_date);
                return;
            }

            tracing::debug!("Generating queued prompt {} for {}", prompt_number, cycle_date);
            
            match Self::generate_single_prompt(
                journal_manager, 
                llm_manager, 
                &cycle_date, 
                prompt_number,
                &personalization_config,
            ).await {
                Ok(()) => {
                    tracing::info!("Successfully generated queued prompt {} for {}", prompt_number, cycle_date);
                }
                Err(e) => {
                    tracing::error!("Failed to generate queued prompt {} for {}: {}", prompt_number, cycle_date, e);
                }
            }
        });
    }

    /// Generate a single prompt (helper method for async generation)
    async fn generate_single_prompt(
        journal_manager: Arc<JournalManager>,
        llm_manager: Arc<LlmManager>,
        cycle_date: &CycleDate,
        prompt_number: u8,
        personalization_config: &PersonalizationConfig,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Create a minimal config for single prompt generation
        let temp_config = crate::config::Config {
            journal: crate::config::JournalConfig {
                journal_directory: "journal".to_string(),
                processing_time: "03:00".to_string(),
                prompt_generation_time: "06:00".to_string(),
                max_prompts_per_day: prompt_number, // Generate up to the requested prompt number
            },
            ..Default::default()
        };
        
        // Use unified generation with checks (since this is typically user-requested)
        Self::generate_prompts_unified(
            journal_manager,
            llm_manager,
            Arc::new(temp_config),
            Arc::new(personalization_config.clone()),
            cycle_date,
            false, // Don't skip checks for user-requested prompts
            Some(prompt_number), // Generate up to this specific prompt number
        ).await.map_err(|e| e.into())
    }

    /// Check if we're past the prompt generation time for today and generate prompts if needed
    async fn check_and_generate_startup_prompts(
        journal_manager: Arc<JournalManager>,
        llm_manager: Arc<LlmManager>,
        config: Arc<Config>,
        personalization_config: Arc<PersonalizationConfig>,
    ) -> Result<(), String> {
        let today = CycleDate::today();
        let now = Local::now();
        
        // First, always check for missing summaries and status files on startup
        tracing::info!("Startup check: Looking for entries that need summaries or status files...");
        
        // Load the LLM model for summary generation
        llm_manager.prepare_for_processing().await.map_err(|e| e.to_string())?;
        let llm_worker = llm_manager.get_worker();
        
        // Generate any missing summaries and status files
        if let Err(e) = Self::generate_missing_summaries(&journal_manager, &llm_worker, &personalization_config).await {
            tracing::warn!("Failed to generate some summaries/status files: {}", e);
            // Continue anyway - this shouldn't block prompt generation
        }
        
        // Parse the configured prompt generation time
        let target_time = NaiveTime::parse_from_str(&config.journal.prompt_generation_time, "%H:%M")
            .map_err(|e| format!("Invalid time format: {}", e))?;
        
        // Check if current time is past the prompt generation time for today
        let current_time = now.time();
        if current_time >= target_time {
            tracing::info!("Startup check: Current time ({}) is past prompt generation time ({})", 
                current_time.format("%H:%M"), target_time.format("%H:%M"));
            
            // Check if we already have prompts for today
            let existing_prompts = Self::count_existing_prompts(&journal_manager, &today).await;
            if existing_prompts == 0 {
                tracing::info!("No prompts found for today, generating them now...");
                Self::generate_prompts_unified(
                    journal_manager,
                    llm_manager,
                    config,
                    personalization_config,
                    &today,
                    false, // Don't skip checks for startup generation
                    None,  // Use default max_prompts_per_day
                ).await?;
            } else {
                tracing::info!("Found {} existing prompts for today, no need to generate", existing_prompts);
            }
        } else {
            tracing::info!("Startup check: Current time ({}) is before prompt generation time ({}), will wait", 
                current_time.format("%H:%M"), target_time.format("%H:%M"));
        }
        
        Ok(())
    }

    /// Generate summaries and status files for entries that don't have them yet
    async fn generate_missing_summaries(
        journal_manager: &Arc<JournalManager>,
        llm_worker: &Arc<crate::llm_worker::LlmWorker>,
        personalization_config: &Arc<PersonalizationConfig>,
    ) -> Result<(), String> {
        // Find entries that need summaries or status files
        let entries_needing_summaries = journal_manager.find_entries_needing_summaries().await.map_err(|e| e.to_string())?;
        let entries_needing_status = journal_manager.find_entries_needing_status().await.map_err(|e| e.to_string())?;
        
        // Combine and deduplicate entries that need processing
        let mut entries_to_process = std::collections::HashSet::new();
        for cycle_date in entries_needing_summaries {
            entries_to_process.insert(cycle_date);
        }
        for cycle_date in entries_needing_status {
            entries_to_process.insert(cycle_date);
        }
        
        if entries_to_process.is_empty() {
            tracing::info!("All entries already have summaries and status files");
            return Ok(());
        }
        
        tracing::info!("Found {} entries needing summaries and/or status files", entries_to_process.len());
        
        // Clone for mutable access
        let mut personalization_config_mut = personalization_config.as_ref().clone();
        
        for cycle_date in entries_to_process {
            // Load the entry content
            let entry_content = match journal_manager.load_entry(&cycle_date).await {
                Ok(Some(entry)) => {
                    entry.content
                }
                Ok(None) => {
                    tracing::warn!("No entry found for {}", cycle_date);
                    continue;
                }
                Err(e) => {
                    tracing::error!("Failed to load entry for {}: {}", cycle_date, e);
                    continue;
                }
            };
            
            // Check what files are missing
            let paths = journal_manager.get_file_paths(&cycle_date);
            let needs_summary = !paths.summary.exists();
            let needs_status = !paths.status.exists();
            
            if needs_summary || needs_status {
                tracing::info!("Processing {} (summary: {}, status: {})", 
                    cycle_date, 
                    if needs_summary { "generating" } else { "exists" },
                    if needs_status { "generating" } else { "exists" }
                );
                
                let (summary, status_update) = llm_worker.generate_summary_with_status_update(&entry_content, &cycle_date, &mut personalization_config_mut).await.map_err(|e| e.to_string())?;
                
                // Save summary if needed
                if needs_summary {
                    journal_manager.save_summary(&summary).await.map_err(|e| e.to_string())?;
                }
                
                // Save status if needed and generated
                if needs_status {
                    if let Some(status) = status_update {
                        journal_manager.save_status(&cycle_date, &status).await.map_err(|e| e.to_string())?;
                        tracing::info!("Summary and status saved for {}", cycle_date);
                    } else {
                        tracing::info!("Summary saved for {} (no status update needed)", cycle_date);
                    }
                } else if let Some(_status) = status_update {
                    // Status file exists but we still updated global status
                    tracing::info!("Summary saved for {} (status exists, global updated)", cycle_date);
                } else {
                    tracing::info!("Summary saved for {} (no status changes)", cycle_date);
                }
            }
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
