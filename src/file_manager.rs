use crate::auth::SessionsData;
use std::path::Path;
use tokio::fs;

/// Manages loading and saving session tokens to/from JSON files
pub struct TokensFileManager {
    file_path: String,
}

impl TokensFileManager {
    /// Create a new token file manager for the given path
    pub fn new(file_path: String) -> Self {
        Self { file_path }
    }

    /// Load sessions from the JSON file
    /// If file doesn't exist, returns a new empty SessionsData
    pub async fn load_sessions(&self) -> Result<SessionsData, Box<dyn std::error::Error + Send + Sync>> {
        // Check if file exists
        if !Path::new(&self.file_path).exists() {
            tracing::info!("Token file not found, creating new one: {}", self.file_path);
            return Ok(SessionsData::new());
        }

        // Read the file
        let content = fs::read_to_string(&self.file_path).await?;
        
        // Parse JSON
        let sessions_data: SessionsData = serde_json::from_str(&content)?;
        
        tracing::info!("Loaded {} device sessions from {}", sessions_data.sessions.len(), self.file_path);
        Ok(sessions_data)
    }

    /// Save sessions to the JSON file
    /// Creates the file if it doesn't exist
    pub async fn save_sessions(&self, sessions_data: &SessionsData) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Serialize to pretty JSON
        let content = serde_json::to_string_pretty(sessions_data)?;
        
        // Write to file
        fs::write(&self.file_path, content).await?;
        
        tracing::info!(" Saved {} device sessions to {}", sessions_data.sessions.len(), self.file_path);
        Ok(())
    }
}
