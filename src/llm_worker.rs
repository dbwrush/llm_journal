use crate::journal::{JournalPrompt, JournalSummary, PromptType};
use crate::cycle_date::CycleDate;
use chrono::Local;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::process::Command;

// Ollama integration for LLM inference
use ollama_rs::Ollama;
use ollama_rs::generation::completion::request::GenerationRequest;
use ollama_rs::models::ModelOptions;

/// LLM Worker for Ollama-based model inference
pub struct LlmWorker {
    model_name: String,
    temperature: f32,
    ollama_client: Ollama,
    is_connected: Arc<Mutex<bool>>,
}

impl LlmWorker {
    pub fn new(model_path: String, temperature: f32, _max_tokens: usize) -> Result<Self, Box<dyn std::error::Error>> {
        // Extract model name from the full path
        // E.g., "C:\...\gpt-oss-20b-MXFP4.gguf" -> "gpt-oss-20b"
        let model_name = Self::extract_model_name(&model_path)?;
        
        // Connect to Ollama using the default (localhost:11434) - most reliable method
        let ollama_client = Ollama::default();
        
        tracing::info!("LLM Worker initialized with Ollama");
        tracing::info!("   Ollama endpoint: localhost:11434 (DEFAULT - LOCAL ONLY)");
        tracing::info!("   Model: {}", model_name);
        tracing::info!("   Temperature: {}", temperature);
        
        Ok(Self {
            model_name,
            temperature,
            ollama_client,
            is_connected: Arc::new(Mutex::new(false)),
        })
    }

    /// Extract model name from file path for Ollama
    fn extract_model_name(model_path: &str) -> Result<String, Box<dyn std::error::Error>> {
        // For now, we'll use a simple mapping. User might need to import the model into Ollama
        if model_path.contains("gpt-oss-20b") {
            Ok("gpt-oss:20b".to_string()) // Use the correct Ollama model name
        } else {
            // Extract filename without extension as fallback
            let filename = std::path::Path::new(model_path)
                .file_stem()
                .ok_or("Invalid model path")?
                .to_str()
                .ok_or("Invalid model path encoding")?;
            Ok(filename.to_string())
        }
    }

    /// Check if Ollama is running and try to start it if needed
    async fn ensure_ollama_running(&self) -> Result<(), Box<dyn std::error::Error>> {
        tracing::info!("Checking if Ollama is running...");
        
        // Try to list models to check if Ollama is accessible
        match self.ollama_client.list_local_models().await {
            Ok(models) => {
                tracing::info!("Ollama is running with {} models available", models.len());
                *self.is_connected.lock().await = true;
                
                // Check if our model is available
                let model_available = models.iter().any(|m| m.name.contains(&self.model_name));
                if !model_available {
                    tracing::warn!("Model '{}' not found in Ollama. Available models:", self.model_name);
                    for model in &models {
                        tracing::warn!("   - {}", model.name);
                    }
                    tracing::warn!("   Please run: ollama pull {}", self.model_name);
                    return Err(format!("Model '{}' not available in Ollama", self.model_name).into());
                }
                
                Ok(())
            }
            Err(_) => {
                tracing::warn!("Ollama not accessible, attempting to start...");
                self.start_ollama().await
            }
        }
    }

    /// Try to start Ollama if it's not running
    async fn start_ollama(&self) -> Result<(), Box<dyn std::error::Error>> {
        tracing::info!(" Attempting to start Ollama...");
        
        // Try to start Ollama in the background
        let mut cmd = if cfg!(target_os = "windows") {
            let mut cmd = Command::new("cmd");
            cmd.args(["/C", "ollama", "serve"]);
            cmd
        } else {
            let mut cmd = Command::new("ollama");
            cmd.arg("serve");
            cmd
        };

        match cmd.spawn() {
            Ok(mut child) => {
                tracing::info!("Started Ollama process (PID: {:?})", child.id());
                
                // Give Ollama time to start
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                
                // Try to connect again
                match self.ollama_client.list_local_models().await {
                    Ok(_) => {
                        tracing::info!("Successfully connected to Ollama");
                        *self.is_connected.lock().await = true;
                        Ok(())
                    }
                    Err(e) => {
                        tracing::error!("Failed to connect to Ollama after starting: {}", e);
                        let _ = child.kill();
                        Err("Could not start or connect to Ollama".into())
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to start Ollama: {}", e);
                tracing::info!("Please install Ollama from https://ollama.ai/ or start it manually");
                Err("Ollama not available and could not be started".into())
            }
        }
    }

    /// Load the model - ensure Ollama is running and model is available
    pub async fn load_model(&self) -> Result<(), Box<dyn std::error::Error>> {
        let is_connected = *self.is_connected.lock().await;
        if is_connected {
            return Ok(());
        }

        self.ensure_ollama_running().await
    }

    /// Check if model is loaded and ready
    pub async fn is_model_loaded(&self) -> bool {
        *self.is_connected.lock().await
    }

    /// Generate text using Ollama
    pub async fn generate_text(&self, prompt: &str, _max_length: usize) -> Result<String, Box<dyn std::error::Error>> {
        // Ensure Ollama is connected
        if !self.is_model_loaded().await {
            tracing::info!("Ollama not connected, connecting now...");
            self.load_model().await?;
        }

        tracing::debug!("Generating text with Ollama (prompt: {} chars)", prompt.len());
        
        // Configure model options - try without num_predict limit first
        let options = ModelOptions::default()
            .temperature(self.temperature);

        // Create generation request with explicit local model specification
        let request = GenerationRequest::new(self.model_name.clone(), prompt.to_string())
            .options(options);

        // Make the request to Ollama
        let start_time = std::time::Instant::now();
        
        match self.ollama_client.generate(request).await {
            Ok(response) => {
                let duration = start_time.elapsed();
                
                tracing::info!("Generated response in {:.2}s ({} chars)", 
                              duration.as_secs_f64(), response.response.len());
                Ok(response.response)
            }
            Err(e) => {
                tracing::error!("Ollama generation failed: {}", e);
                // Reset connection status on error
                *self.is_connected.lock().await = false;
                Err(format!("Ollama generation failed: {}", e).into())
            }
        }
    }
    
    /// Generate a summary for a journal entry
    pub async fn generate_summary(
        &self, 
        entry_content: &str, 
        cycle_date: &CycleDate,
        personalization_config: &crate::personalization::PersonalizationConfig,
    ) -> Result<JournalSummary, Box<dyn std::error::Error>> {
        let prompt = personalization_config.prompts.get_summary_prompt(entry_content);
        
        let summary = self.generate_text(&prompt, 100).await?;
        
        Ok(JournalSummary {
            cycle_date: *cycle_date,
            summary: summary.trim().to_string(),
            generated_at: Local::now(),
        })
    }
    
    /// Generate both summary and status update for a journal entry
    pub async fn generate_summary_with_status_update(
        &self,
        entry_content: &str,
        cycle_date: &CycleDate,
        personalization_config: &mut crate::personalization::PersonalizationConfig,
    ) -> Result<(JournalSummary, Option<String>), Box<dyn std::error::Error>> {
        // First generate the summary
        let summary = self.generate_summary(entry_content, cycle_date, personalization_config).await?;
        
        // Generate status update based on the entry and current status
        let status_update = self.generate_status_update(entry_content, personalization_config).await?;
        
        // Update the personalization config with new status
        if let Some(ref new_status) = status_update {
            personalization_config.update_status(new_status.clone())?;
        }
        
        Ok((summary, status_update))
    }
    
    /// Generate a status update based on journal entry and current status
    async fn generate_status_update(
        &self,
        entry_content: &str,
        personalization_config: &crate::personalization::PersonalizationConfig,
    ) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let current_status = personalization_config.get_current_status()
            .map(|s| s.as_str())
            .unwrap_or("No previous status recorded.");
        
        let prompt = format!(
            r#"Based on this journal entry and the current status, update the user's ongoing life circumstances. Focus on significant changes, ongoing situations, emotional states, relationships, work/health updates, and challenges/projects that should be remembered for future context.

CURRENT STATUS:
{}

TODAY'S JOURNAL ENTRY:
{}

Please provide an updated status summary that:
1. Preserves important ongoing situations from current status
2. Incorporates significant new developments from today's entry  
3. Removes outdated information
4. Focuses on context that will be valuable for future journal prompts
5. Keeps it concise but informative (3-5 sentences)

If today's entry doesn't contain significant status changes, respond with "NO_UPDATE_NEEDED".

Updated Status:"#,
            current_status,
            entry_content
        );
        
        let response = self.generate_text(&prompt, 200).await?;
        let response = response.trim();
        
        if response == "NO_UPDATE_NEEDED" || response.is_empty() {
            tracing::info!(" No status update needed for today's entry");
            Ok(None)
        } else {
            tracing::info!("Generated status update ({} characters)", response.len());
            Ok(Some(response.to_string()))
        }
    }

    /// Generate a journal prompt based on context
    pub async fn generate_prompt(
        &self,
        cycle_date: &CycleDate,
        context: &[String],
        prompt_number: u8,
        prompt_type: PromptType,
        personalization_config: &crate::personalization::PersonalizationConfig,
    ) -> Result<JournalPrompt, Box<dyn std::error::Error>> {
        let context_str = context.join("\n\n");
        
        // Enrich context with user profile and style information
        let enriched_context = personalization_config.enrich_context(&context_str);
        
        let system_prompt = personalization_config.prompts.get_prompt_template(&prompt_type, &enriched_context);

        // Add variation for multiple prompts
        let variation_suffix = personalization_config.prompts.get_variation_suffix(prompt_number);
        let variation_prompt = if variation_suffix.is_empty() {
            system_prompt
        } else {
            format!("{}{}", system_prompt, variation_suffix)
        };
        
        let generated_prompt = self.generate_text(&variation_prompt, 150).await?;
        
        Ok(JournalPrompt {
            cycle_date: *cycle_date,
            prompt: generated_prompt.trim().to_string(),
            prompt_number,
            generated_at: Local::now(),
            prompt_type,
        })
    }
}

/// Manages the lifecycle of the LLM worker
pub struct LlmManager {
    worker: Arc<LlmWorker>,
}

impl LlmManager {
    pub fn new(model_path: String) -> Result<Self, Box<dyn std::error::Error>> {
        let worker = Arc::new(LlmWorker::new(model_path, 0.7, 512)?);
        Ok(Self { worker })
    }

    /// Load model for processing
    pub async fn prepare_for_processing(&self) -> Result<(), Box<dyn std::error::Error>> {
        if !self.worker.is_model_loaded().await {
            self.worker.load_model().await?;
        }
        Ok(())
    }

    /// Get worker reference for generation tasks
    pub fn get_worker(&self) -> Arc<LlmWorker> {
        Arc::clone(&self.worker)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_llm_worker_creation() {
        let worker = LlmWorker::new("gpt-oss-20b".to_string(), 0.7, 512);
        assert!(worker.is_ok());
        
        let worker = worker.unwrap();
        assert!(!worker.is_model_loaded().await);
    }

    #[test]
    fn test_model_name_extraction() {
        let test_cases = vec![
            ("C:\\Users\\test\\.lmstudio\\models\\gpt-oss-20b-GGUF\\gpt-oss-20b-MXFP4.gguf", "gpt-oss-20b"),
            ("/home/user/models/llama2.gguf", "llama2"),
            ("model.gguf", "model"),
        ];

        for (input, expected) in test_cases {
            let result = LlmWorker::extract_model_name(input).unwrap();
            if input.contains("gpt-oss-20b") {
                assert_eq!(result, expected);
            } else {
                assert!(result.contains(expected));
            }
        }
    }
}
