use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

// Forward declare so we can use it in AuthManager
use crate::file_manager::TokensFileManager;

/// Represents a pending authentication request
#[derive(Debug, Clone)]
pub struct PendingAuth {
    pub passcode: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub device_name: Option<String>,
    pub is_physical_device: bool,
}

/// Represents an authentication session (now persistent)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub token: String,
    pub device_name: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_used: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    pub is_physical_device: bool,
}

/// Collection of all persistent sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionsData {
    pub sessions: Vec<Session>,
    pub version: u32,
}

/// Manages authentication state
#[derive(Debug)]
pub struct AuthManager {
    /// Pending authentication requests (passcode -> PendingAuth)
    pub pending_auths: Arc<RwLock<HashMap<String, PendingAuth>>>,
    /// Valid session tokens (token -> Session)
    pub sessions: Arc<RwLock<HashMap<String, Session>>>,
}

impl SessionsData {
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
            version: 1,
        }
    }
}

impl AuthManager {
    pub fn new() -> Self {
        tracing::info!("Authentication system initialized");
        tracing::info!("   Each device will get a unique secure passcode");
        
        Self {
            pending_auths: Arc::new(RwLock::new(HashMap::new())),
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Load persistent sessions from SessionsData
    pub async fn load_sessions(&self, sessions_data: &SessionsData) {
        let mut sessions = self.sessions.write().await;
        sessions.clear();
        
        for session in &sessions_data.sessions {
            sessions.insert(session.token.clone(), session.clone());
        }
        
        tracing::info!("Loaded {} persistent device sessions", sessions.len());
    }

    /// Get current sessions as SessionsData for saving
    pub async fn get_sessions_data(&self) -> SessionsData {
        let sessions = self.sessions.read().await;
        let sessions_vec: Vec<Session> = sessions.values().cloned().collect();
        
        SessionsData {
            sessions: sessions_vec,
            version: 1,
        }
    }

    /// Save current sessions to file (auto-save helper)
    pub async fn save_sessions_to_file(&self, tokens_manager: &TokensFileManager) {
        let sessions_data = self.get_sessions_data().await;
        if let Err(e) = tokens_manager.save_sessions(&sessions_data).await {
            // Log error but don't fail the authentication
            tracing::warn!("Warning: Could not save sessions to file: {}", e);
        }
    }

    /// Generates a new passcode for device authentication
    pub async fn create_auth_request(&self, device_name: Option<String>, is_physical_device: bool) -> String {
        let passcode = generate_secure_passcode();
        let auth_request = PendingAuth {
            passcode: passcode.clone(),
            created_at: chrono::Utc::now(),
            device_name: device_name.clone(),
            is_physical_device,
        };
        
        // Store the pending auth
        self.pending_auths.write().await.insert(passcode.clone(), auth_request);
        
        tracing::info!(" New authentication request:");
        tracing::info!("   Device: {:?} (Physical: {})", 
                     device_name.as_deref().unwrap_or("Unknown"), 
                     is_physical_device);
        tracing::info!("   Passcode: {}", passcode);
        tracing::info!("   (This code expires in 10 minutes)");
        
        passcode
    }

    /// Validates a passcode and creates a new session if valid
    pub async fn authenticate(&self, passcode: &str, device_name: Option<String>, is_physical_device: bool) -> Option<String> {
        // Check if this passcode exists and is still valid
        let mut pending_auths = self.pending_auths.write().await;
        
        if let Some(auth_request) = pending_auths.get(passcode) {
            // Check if the code has expired (10 minutes)
            let now = chrono::Utc::now();
            let age = now.signed_duration_since(auth_request.created_at);
            
            if age.num_minutes() > 10 {
                // Expired - remove it
                pending_auths.remove(passcode);
                tracing::warn!(" Authentication code expired");
                return None;
            }
            
            // Valid code - create session and remove the pending auth
            let now = chrono::Utc::now();
            let token = Uuid::new_v4().to_string();
            let session = Session {
                token: token.clone(),
                device_name: device_name.clone(),
                created_at: now,
                last_used: now,
                is_physical_device,
            };
            
            // Remove the used passcode
            pending_auths.remove(passcode);
            drop(pending_auths); // Release the lock
            
            // Add the session
            self.sessions.write().await.insert(token.clone(), session);
            tracing::info!(" New device authenticated: {:?}", device_name.as_deref().unwrap_or("Unknown"));
            Some(token)
        } else {
            tracing::warn!(" Invalid passcode attempt");
            None
        }
    }

    /// Validates a session token
    pub async fn validate_session(&self, token: &str) -> bool {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(token) {
            // Update last_used timestamp
            session.last_used = chrono::Utc::now();
            true
        } else {
            false
        }
    }
    
    /// Get session information including device type
    pub async fn get_session_info(&self, token: &str) -> Option<Session> {
        let sessions = self.sessions.read().await;
        sessions.get(token).cloned()
    }

    /// Removes a session (for logout or invalid tokens)
    pub async fn remove_session(&self, token: &str) {
        self.sessions.write().await.remove(token);
    }
}

/// Generates a cryptographically secure 256-bit passcode
fn generate_secure_passcode() -> String {
    use rand::RngCore;
    let mut rng = rand::thread_rng();
    
    // Generate 32 bytes (256 bits) of random data
    let mut bytes = [0u8; 32];
    rng.fill_bytes(&mut bytes);
    
    // Convert to base64 for easier copying (but we'll use hex for terminal display)
    // Base64 would be 43 characters, hex is 64 characters
    // Let's use hex for better readability in terminal
    hex::encode(bytes)
}
