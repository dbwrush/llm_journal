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
    max_tokens: usize,
    ollama_client: Ollama,
    is_connected: Arc<Mutex<bool>>,
}

impl LlmWorker {
    pub fn new(model_path: String, temperature: f32, max_tokens: usize) -> Result<Self, Box<dyn std::error::Error>> {
        // Extract model name from the full path
        // E.g., "C:\...\gpt-oss-20b-MXFP4.gguf" -> "gpt-oss-20b"
        let model_name = Self::extract_model_name(&model_path)?;
        
        // Connect to Ollama using the default (localhost:11434) - most reliable method
        let ollama_client = Ollama::default();
        
        tracing::info!("🔧 LLM Worker initialized with Ollama");
        tracing::info!("   🏠 Ollama endpoint: localhost:11434 (DEFAULT - LOCAL ONLY)");
        tracing::info!("   🤖 Model: {}", model_name);
        tracing::info!("   🌡️  Temperature: {}", temperature);
        tracing::info!("   🎯 Max tokens: {}", max_tokens);
        
        Ok(Self {
            model_name,
            temperature,
            max_tokens,
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
        tracing::info!("🔍 Checking if Ollama is running...");
        
        // Try to list models to check if Ollama is accessible
        match self.ollama_client.list_local_models().await {
            Ok(models) => {
                tracing::info!("✅ Ollama is running with {} models available", models.len());
                *self.is_connected.lock().await = true;
                
                // Check if our model is available
                let model_available = models.iter().any(|m| m.name.contains(&self.model_name));
                if !model_available {
                    tracing::warn!("⚠️  Model '{}' not found in Ollama. Available models:", self.model_name);
                    for model in &models {
                        tracing::warn!("   - {}", model.name);
                    }
                    tracing::warn!("   Please run: ollama pull {}", self.model_name);
                    return Err(format!("Model '{}' not available in Ollama", self.model_name).into());
                }
                
                Ok(())
            }
            Err(_) => {
                tracing::warn!("⚠️  Ollama not accessible, attempting to start...");
                self.start_ollama().await
            }
        }
    }

    /// Try to start Ollama if it's not running
    async fn start_ollama(&self) -> Result<(), Box<dyn std::error::Error>> {
        tracing::info!("� Attempting to start Ollama...");
        
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
                tracing::info!("🔄 Started Ollama process (PID: {:?})", child.id());
                
                // Give Ollama time to start
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                
                // Try to connect again
                match self.ollama_client.list_local_models().await {
                    Ok(_) => {
                        tracing::info!("✅ Successfully connected to Ollama");
                        *self.is_connected.lock().await = true;
                        Ok(())
                    }
                    Err(e) => {
                        tracing::error!("❌ Failed to connect to Ollama after starting: {}", e);
                        let _ = child.kill();
                        Err("Could not start or connect to Ollama".into())
                    }
                }
            }
            Err(e) => {
                tracing::error!("❌ Failed to start Ollama: {}", e);
                tracing::info!("💡 Please install Ollama from https://ollama.ai/ or start it manually");
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
    pub async fn generate_text(&self, prompt: &str, max_length: usize) -> Result<String, Box<dyn std::error::Error>> {
        // Ensure Ollama is connected
        if !self.is_model_loaded().await {
            tracing::info!("🔄 Ollama not connected, connecting now...");
            self.load_model().await?;
        }

        let effective_max_length = if max_length > 0 { max_length } else { self.max_tokens };
        
        tracing::info!("🤖 Generating text with Ollama");
        tracing::info!("   📝 Prompt length: {} chars", prompt.len());
        tracing::info!("   🎯 Max output tokens: {}", effective_max_length);
        tracing::info!("   🌡️  Temperature: {}", self.temperature);
        tracing::info!("   📄 Model: {}", self.model_name);
        
        // Configure model options - try without num_predict limit first
        let options = ModelOptions::default()
            .temperature(self.temperature);

        // Create generation request with explicit local model specification
        let request = GenerationRequest::new(self.model_name.clone(), prompt.to_string())
            .options(options);

        // Debug: Log the exact request details
        tracing::info!("🔍 Request details:");
        tracing::info!("   Model name: '{}'", self.model_name);
        tracing::info!("   Prompt: '{}'", prompt);
        tracing::info!("   Ollama endpoint: http://localhost:11434");

        // Make the request to Ollama
        tracing::info!("⚡ Sending request to Ollama...");
        let start_time = std::time::Instant::now();
        
        match self.ollama_client.generate(request).await {
            Ok(response) => {
                let duration = start_time.elapsed();
                
                // Debug: Log the full response structure
                tracing::info!("🔍 Full response debug:");
                tracing::info!("   Response text: '{}'", response.response);
                tracing::info!("   Response length: {} chars", response.response.len());
                tracing::info!("   Model used: '{}'", response.model);
                tracing::info!("   Done: {}", response.done);
                tracing::info!("   Context present: {}", response.context.is_some());
                
                tracing::info!("✅ Generated response in {:.2}s ({} chars)", 
                              duration.as_secs_f64(), response.response.len());
                Ok(response.response)
            }
            Err(e) => {
                tracing::error!("❌ Ollama generation failed: {}", e);
                // Reset connection status on error
                *self.is_connected.lock().await = false;
                Err(format!("Ollama generation failed: {}", e).into())
            }
        }
    }
    
    /// Generate a summary for a journal entry
    pub async fn generate_summary(&self, entry_content: &str, cycle_date: &CycleDate) -> Result<JournalSummary, Box<dyn std::error::Error>> {
        let prompt = format!(
            "Please summarize the following journal entry in 2-3 sentences, focusing on key emotions, events, and insights:\n\n{}\n\nSummary:",
            entry_content
        );
        
        let summary = self.generate_text(&prompt, 100).await?;
        
        Ok(JournalSummary {
            cycle_date: *cycle_date,
            summary: summary.trim().to_string(),
            generated_at: Local::now(),
        })
    }

    /// Generate a journal prompt based on context
    pub async fn generate_prompt(
        &self,
        cycle_date: &CycleDate,
        context: &[String],
        prompt_number: u8,
        prompt_type: PromptType,
    ) -> Result<JournalPrompt, Box<dyn std::error::Error>> {
        let context_str = context.join("\n\n");
        
        let system_prompt = match prompt_type {
            PromptType::Daily => {
                format!(
                    "Based on the following journal summaries from the past week, create an insightful and thought-provoking journal prompt for today. The prompt should help the person reflect on patterns, growth, or connections to recent experiences:\n\n{}\n\nToday's journal prompt:",
                    context_str
                )
            }
            PromptType::WeeklyReflection => {
                format!(
                    "Based on the following journal entries from the past week, create a reflective prompt that encourages deeper weekly reflection on themes, patterns, growth, and lessons learned:\n\n{}\n\nWeekly reflection prompt:",
                    context_str
                )
            }
            PromptType::MonthlyReflection => {
                format!(
                    "Based on the following weekly reflections from the past month, create a comprehensive monthly reflection prompt that explores broader patterns, achievements, challenges, and personal growth:\n\n{}\n\nMonthly reflection prompt:",
                    context_str
                )
            }
            PromptType::YearlyReflection => {
                format!(
                    "Based on the following monthly reflections from the past year, create a profound yearly reflection prompt that encourages deep introspection on personal transformation, major themes, life lessons, and future aspirations:\n\n{}\n\nYearly reflection prompt:",
                    context_str
                )
            }
        };

        // Add variation for multiple prompts
        let variation_prompt = match prompt_number {
            1 => system_prompt,
            2 => format!("{}\n\nCreate a different perspective or angle for this prompt:", system_prompt),
            3 => format!("{}\n\nCreate a third unique approach to this reflection:", system_prompt),
            n if n > 3 => format!("{}\n\nCreate another unique and creative approach to this reflection (variation #{}):", system_prompt, n),
            _ => return Err("Invalid prompt number".into()),
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
