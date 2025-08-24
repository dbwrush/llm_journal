use crate::config::Config;
use crate::cycle_date::CycleDate;
use crate::journal::{JournalManager, PromptType};
use crate::llm_worker::LlmManager;
use crate::personalization::PersonalizationConfig;
use std::sync::Arc;
use tokio_cron_scheduler::{Job, JobScheduler};

/// Background processor that handles nightly LLM tasks
pub struct JournalProcessor {
    journal_manager: Arc<JournalManager>,
    llm_manager: Arc<LlmManager>,
    config: Arc<Config>,
    scheduler: JobScheduler,
}

impl JournalProcessor {
    pub async fn new(
        journal_manager: Arc<JournalManager>,
        llm_manager: Arc<LlmManager>,
        config: Arc<Config>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let scheduler = JobScheduler::new().await?;
        
        Ok(Self {
            journal_manager,
            llm_manager,
            config,
            scheduler,
        })
    }

    /// Start the background processing scheduler
    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        tracing::info!("Starting journal background processor...");
        
        // Parse the processing time from config (e.g., "03:00")
        let time_parts: Vec<&str> = self.config.journal.processing_time.split(':').collect();
        if time_parts.len() != 2 {
            return Err("Invalid processing_time format. Use HH:MM format.".into());
        }
        
        let hour: u32 = time_parts[0].parse()?;
        let minute: u32 = time_parts[1].parse()?;
        
        if hour >= 24 || minute >= 60 {
            return Err("Invalid processing_time. Hour must be 0-23, minute must be 0-59.".into());
        }

        // Create cron expression for daily processing
        let cron_expression = format!("0 {} {} * * *", minute, hour);
        
        // Clone Arc references for the job closure
        let journal_manager = Arc::clone(&self.journal_manager);
        let llm_manager = Arc::clone(&self.llm_manager);
        let config = Arc::clone(&self.config);
        
        let job = Job::new_async(cron_expression.as_str(), move |_uuid, _l| {
            let journal_manager = Arc::clone(&journal_manager);
            let llm_manager = Arc::clone(&llm_manager);
            let config = Arc::clone(&config);
            
            Box::pin(async move {
                if let Err(e) = process_journal_tasks(journal_manager, llm_manager, config).await {
                    tracing::error!("Error in journal processing: {}", e);
                } else {
                    tracing::info!("Journal processing completed successfully");
                }
            })
        })?;

        self.scheduler.add(job).await?;
        self.scheduler.start().await?;
        
        tracing::info!(
            "Journal processor scheduled to run daily at {}",
            self.config.journal.processing_time
        );
        
        Ok(())
    }

    /// Stop the background processor
    pub async fn stop(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        tracing::info!("🔄 Stopping journal background processor...");
        self.scheduler.shutdown().await?;
        tracing::info!("✅ Journal processor stopped");
        Ok(())
    }

    /// Run processing tasks manually (for testing or immediate execution)
    pub async fn run_processing_now(&self) -> Result<(), Box<dyn std::error::Error>> {
        tracing::info!("🔄 Running journal processing manually...");
        
        process_journal_tasks(
            Arc::clone(&self.journal_manager),
            Arc::clone(&self.llm_manager),
            Arc::clone(&self.config),
        ).await?;
        
        tracing::info!("✅ Manual journal processing completed");
        Ok(())
    }
}

/// Core processing function that runs the nightly tasks
async fn process_journal_tasks(
    journal_manager: Arc<JournalManager>,
    llm_manager: Arc<LlmManager>,
    config: Arc<Config>,
) -> Result<(), String> {
    tracing::info!("🌙 Starting nightly journal processing...");
    
    // Load personalization configuration 
    let mut personalization_config = PersonalizationConfig::load(&config.journal.journal_directory)
        .map_err(|e| format!("Failed to load personalization config: {}", e))?;
    
    // Step 1: Load the LLM model
    tracing::info!("🔄 Loading LLM model...");
    llm_manager.prepare_for_processing().await.map_err(|e| e.to_string())?;
    let llm_worker = llm_manager.get_worker();
    
    // Step 2: Generate summaries for entries that don't have them
    tracing::info!("🔄 Generating summaries for entries...");
    let entries_needing_summaries = journal_manager.find_entries_needing_summaries().await.map_err(|e| e.to_string())?;
    
    for cycle_date in entries_needing_summaries {
        // Convert the result to avoid Send issues
        let entry_content = match journal_manager.load_entry(&cycle_date).await {
            Ok(Some(entry)) => {
                tracing::info!("📝 Generating summary for {}", cycle_date);
                entry.content
            }
            Ok(None) => {
                tracing::warn!("⚠️  No entry found for {}", cycle_date);
                continue;
            }
            Err(e) => {
                tracing::error!("❌ Failed to load entry for {}: {}", cycle_date, e);
                continue;
            }
        };
        
        let (summary, status_update) = llm_worker.generate_summary_with_status_update(&entry_content, &cycle_date, &mut personalization_config).await.map_err(|e| e.to_string())?;
        journal_manager.save_summary(&summary).await.map_err(|e| e.to_string())?;
        
        if let Some(status) = status_update {
            tracing::info!("✅ Summary and status update saved for {}", cycle_date);
            tracing::debug!("📄 Status update: {}", status);
        } else {
            tracing::info!("✅ Summary saved for {} (no status update)", cycle_date);
        }
    }
    
    // Step 3: Generate prompts for tomorrow
    let tomorrow = CycleDate::today().next_day();
    tracing::info!("Generating prompts for {}", tomorrow);
    
    // Determine prompt type based on tomorrow's position in the cycle
    let prompt_type = if tomorrow.is_first_day_of_year() {
        PromptType::YearlyReflection
    } else if tomorrow.is_first_day_of_month() {
        PromptType::MonthlyReflection
    } else if tomorrow.is_first_day_of_week() {
        PromptType::WeeklyReflection
    } else {
        PromptType::Daily
    };
    
    // Get context for prompt generation
    let context = journal_manager.get_context_for_prompt(&tomorrow).await.map_err(|e| e.to_string())?;
    
    // Generate multiple prompts
    for prompt_number in 1..=config.journal.max_prompts_per_day {
        tracing::info!("📝 Generating prompt {} for {}", prompt_number, tomorrow);
        
        let prompt = llm_worker.generate_prompt(
            &tomorrow,
            &context,
            prompt_number,
            prompt_type.clone(),
            &personalization_config,
        ).await.map_err(|e| e.to_string())?;
        
        journal_manager.save_prompt(&prompt).await.map_err(|e| e.to_string())?;
        
        tracing::info!("✅ Prompt {} saved for {}", prompt_number, tomorrow);
    }
    
    tracing::info!("✅ Nightly journal processing completed successfully");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_processor_creation() {
        let temp_dir = TempDir::new().unwrap();
        let journal_manager = Arc::new(JournalManager::new(temp_dir.path()));
        let llm_manager = Arc::new(LlmManager::new("test_model".to_string()).unwrap());
        let config = Arc::new(Config::default());
        
        let processor = JournalProcessor::new(journal_manager, llm_manager, config).await;
        assert!(processor.is_ok());
    }
}
