use serde::Deserialize;
use std::fs;

/// Application configuration
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Server configuration
    pub server: ServerConfig,
    /// File paths
    pub files: FileConfig,
    /// Authentication settings
    pub auth: AuthConfig,
    /// Journal settings
    pub journal: JournalConfig,
    /// LLM settings
    pub llm: LlmConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Port to listen on
    pub port: u16,
    /// Host to bind to
    pub host: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileConfig {
    /// Path to tokens/sessions file
    pub tokens_file: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthConfig {
    /// Session duration in seconds (default: 30 days)
    pub session_duration_seconds: u64,
    /// Passcode expiration in seconds (default: 10 minutes)
    pub passcode_expiration_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JournalConfig {
    /// Directory to store journal files
    pub journal_directory: String,
    /// Time to run nightly processing (in 24-hour format, e.g., "03:00")
    pub processing_time: String,
    /// Time to generate daily prompts (in 24-hour format, e.g., "06:00")
    pub prompt_generation_time: String,
    /// Maximum number of prompts to generate per day
    pub max_prompts_per_day: u8,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LlmConfig {
    /// Path to the model file
    pub model_path: String,
    /// Context length for the model
    pub context_length: usize,
    /// Temperature for generation
    pub temperature: f32,
    /// Maximum tokens to generate
    pub max_tokens: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                port: 3000,
                host: "0.0.0.0".to_string(),
            },
            files: FileConfig {
                tokens_file: "tokens.json".to_string(),
            },
            auth: AuthConfig {
                session_duration_seconds: 31536000, // 1 year (365 days)
                passcode_expiration_seconds: 600,   // 10 minutes
            },
            journal: JournalConfig {
                journal_directory: "journal_entries".to_string(),
                processing_time: "03:00".to_string(),
                prompt_generation_time: "06:00".to_string(),
                max_prompts_per_day: 3,
            },
            llm: LlmConfig {
                model_path: "models/gpt-oss-20b.gguf".to_string(),
                context_length: 128000,
                temperature: 0.7,
                max_tokens: 512,
            },
        }
    }
}

impl Config {
    /// Load configuration from file, falling back to defaults
    pub fn load() -> Self {
        match fs::read_to_string("config.toml") {
            Ok(content) => {
                match toml::from_str(&content) {
                    Ok(config) => {
                        tracing::info!("📁 Loaded configuration from config.toml");
                        config
                    }
                    Err(e) => {
                        tracing::warn!("⚠️  Invalid config.toml format: {}, using defaults", e);
                        Self::default()
                    }
                }
            }
            Err(_) => {
                tracing::info!("📁 No config.toml found, using default configuration");
                Self::default()
            }
        }
    }
    
    /// Create a sample configuration file
    pub fn create_sample_config() -> Result<(), Box<dyn std::error::Error>> {
        let sample_config = r#"# LLM Journal Configuration

[server]
port = 3000
host = "0.0.0.0"

[files]
tokens_file = "tokens.json"

[auth]
# Session duration in seconds (1 year)
session_duration_seconds = 31536000
# Passcode expiration in seconds (10 minutes)  
passcode_expiration_seconds = 600

[journal]
# Directory to store journal files
journal_directory = "journal_entries"
# Time to run nightly processing (24-hour format)
processing_time = "03:00"
# Time to generate daily prompts (24-hour format)
prompt_generation_time = "06:00"
# Maximum number of prompts to generate per day
max_prompts_per_day = 3

[llm]
# Model identifier for HuggingFace Hub
model_name = "microsoft/gpt-oss-20b"
# Maximum tokens for summaries
summary_max_tokens = 100
# Maximum tokens for prompts
prompt_max_tokens = 150
# Use GPU acceleration (requires CUDA)
use_gpu = true
"#;
        
        fs::write("config.toml.example", sample_config)?;
        tracing::info!("📝 Created config.toml.example file");
        Ok(())
    }
}
