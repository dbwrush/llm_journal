mod auth;
mod config;
mod cycle_date;
mod file_manager;
mod handlers;
mod journal;
// mod journal_processor; // TODO: Re-enable when scheduling is implemented
mod llm_worker;
mod prompt_generator;

use std::sync::Arc;
use tower_http::trace::TraceLayer;

use auth::AuthManager;
use config::Config;
use file_manager::TokensFileManager;
use handlers::create_routes;
// use journal::JournalManager; // TODO: Re-enable when journal is complete
// use journal_processor::JournalProcessor; // TODO: Re-enable when scheduling is implemented  
// use llm_worker::LlmManager; // TODO: Re-enable when LLM integration is ready

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub auth_manager: Arc<AuthManager>,
    pub tokens_file_manager: Arc<TokensFileManager>,
    pub config: Arc<Config>,
    pub journal_manager: Arc<journal::JournalManager>,
    pub prompt_generator: Option<Arc<prompt_generator::PromptGenerator>>,
}

#[tokio::main]
async fn main() {
    // Initialize tracing for logging
    tracing_subscriber::fmt::init();

    // Load configuration
    let config = Arc::new(Config::load());
    
    // Create sample config if it doesn't exist
    if let Err(e) = Config::create_sample_config() {
        tracing::warn!("Could not create sample config: {}", e);
    }

    // Create authentication manager and load persistent sessions
    let auth_manager = Arc::new(AuthManager::new());
    let tokens_file_manager = Arc::new(TokensFileManager::new(config.files.tokens_file.clone()));
    
    // Initialize journal manager
    let journal_manager = Arc::new(journal::JournalManager::new(&config.journal.journal_directory));
    if let Err(e) = journal_manager.ensure_directories().await {
        tracing::warn!("⚠️  Could not create journal directories: {}", e);
    } else {
        tracing::info!("📁 Journal directory ready: {}", config.journal.journal_directory);
    }
    
    // Initialize LLM manager
    // TODO: Re-enable when LLM integration is ready
    /*
    let llm_manager = match llm_worker::LlmManager::new(config.llm.model_name.clone()) {
        Ok(manager) => {
            tracing::info!("🤖 LLM manager initialized for model: {}", config.llm.model_name);
            Arc::new(manager)
        }
        Err(e) => {
            tracing::error!("❌ Failed to initialize LLM manager: {}", e);
            tracing::warn!("⚠️  Journal prompts will not be generated automatically");
            return;
        }
    };
    */
    
    // Initialize journal processor for background tasks
    // TODO: Re-enable when scheduling issues are resolved
    /*
    let journal_processor = match journal_processor::JournalProcessor::new(
        journal_manager.clone(),
        llm_manager.clone(),
        config.clone(),
    ).await {
        Ok(processor) => {
            tracing::info!("⏰ Journal processor initialized");
            processor
        }
        Err(e) => {
            tracing::error!("❌ Failed to initialize journal processor: {}", e);
            return;
        }
    };
    
    // Start the background journal processing
    if let Err(e) = journal_processor.start().await {
        tracing::error!("❌ Failed to start journal processor: {}", e);
        tracing::warn!("⚠️  Background journal processing disabled");
    }
    */
    
    match tokens_file_manager.load_sessions().await {
        Ok(sessions_data) => {
            auth_manager.load_sessions(&sessions_data).await;
            tracing::info!("✅ Successfully loaded device sessions");
        }
        Err(e) => {
            tracing::warn!("⚠️  Error loading device sessions: {}", e);
            tracing::info!("   Starting with empty session list...");
        }
    }

    // Initialize LLM manager and prompt generator (without startup test)
    let prompt_generator = match crate::llm_worker::LlmManager::new(config.llm.model_path.clone()) {
        Ok(llm_manager) => {
            let llm_manager = Arc::new(llm_manager);
            
            // Initialize prompt generator
            let prompt_generator = Arc::new(crate::prompt_generator::PromptGenerator::new(
                journal_manager.clone(),
                llm_manager.clone(),
                config.clone(),
            ));
            
            // Start the prompt generator service
            if let Err(e) = prompt_generator.start().await {
                tracing::error!("❌ Failed to start prompt generator: {}", e);
                None
            } else {
                tracing::info!("🎯 Prompt generator service started successfully");
                Some(prompt_generator)
            }
        }
        Err(e) => {
            tracing::error!("❌ Failed to create LLM manager: {}", e);
            None
        }
    };

    // Create shared application state
    let app_state = AppState {
        auth_manager: auth_manager.clone(),
        tokens_file_manager: tokens_file_manager.clone(),
        config: config.clone(),
        journal_manager: journal_manager.clone(),
        prompt_generator,
    };

    // Build our application with clean, simple routes
    let app = create_routes()
        .with_state(app_state.clone())
        // Add tracing middleware
        .layer(TraceLayer::new_for_http());

    // Run our app with hyper, listening on configured port
    let bind_address = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&bind_address).await.unwrap();
    tracing::info!("🚀 Server running on http://{}", bind_address);
    tracing::info!("   Press Ctrl+C to shutdown gracefully");
    
    // Set up graceful shutdown
    let auth_manager_shutdown = app_state.auth_manager.clone();
    let tokens_manager_shutdown = app_state.tokens_file_manager.clone();
    
    let shutdown_signal = async move {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
        
        tracing::info!("🛑 Shutdown signal received, saving data...");
        
        // Save current sessions before shutdown
        let sessions_data = auth_manager_shutdown.get_sessions_data().await;
        if let Err(e) = tokens_manager_shutdown.save_sessions(&sessions_data).await {
            tracing::warn!("⚠️  Warning: Could not save sessions during shutdown: {}", e);
        } else {
            tracing::info!("💾 Sessions saved successfully");
        }
        
        tracing::info!("👋 Goodbye!");
    };

    // Run the server with graceful shutdown
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal)
        .await
        .unwrap();
}