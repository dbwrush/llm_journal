mod auth;
mod config;
mod cycle_date;
mod file_manager;
mod handlers;
mod journal;
mod journal_processor;
mod llm_worker;
mod personalization;
mod prompt_generator;
mod prompts;

use std::sync::Arc;
use tower_http::trace::TraceLayer;

use auth::AuthManager;
use config::Config;
use file_manager::TokensFileManager;
use handlers::create_routes;
use journal::JournalManager;
use journal_processor::JournalProcessor;
use llm_worker::LlmManager;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub auth_manager: Arc<AuthManager>,
    pub tokens_file_manager: Arc<TokensFileManager>,
    pub config: Arc<Config>,
    pub journal_manager: Arc<journal::JournalManager>,
    pub prompt_generator: Option<Arc<prompt_generator::PromptGenerator>>,
    pub personalization_config: Arc<personalization::PersonalizationConfig>,
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
    
    // Load personalization configuration (prompts, profile, style)
    let personalization_config = match personalization::PersonalizationConfig::load(&config.journal.journal_directory) {
        Ok(config) => {
            tracing::info!("📝 Personalization configuration loaded successfully");
            Arc::new(config)
        }
        Err(e) => {
            tracing::error!("❌ Failed to load personalization configuration: {}", e);
            std::process::exit(1);
        }
    };
    
    // Create example prompts file for user reference
    if let Err(e) = prompts::PromptsConfig::create_example("prompts") {
        tracing::warn!("⚠️  Could not create example prompts file: {}", e);
    }
    
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

    // Initialize LLM manager first (shared by journal processor and prompt generator)
    let llm_manager = match LlmManager::new(config.llm.model_path.clone()) {
        Ok(manager) => {
            tracing::info!("🤖 LLM manager initialized");
            Arc::new(manager)
        }
        Err(e) => {
            tracing::error!("❌ Failed to initialize LLM manager: {}", e);
            tracing::warn!("⚠️  Journal processing and prompts will not be generated automatically");
            std::process::exit(1);
        }
    };

    // Initialize journal processor for background tasks
    let journal_processor = match JournalProcessor::new(
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
            std::process::exit(1);
        }
    };
    
    // Start the background journal processing
    if let Err(e) = journal_processor.start().await {
        tracing::error!("❌ Failed to start journal processor: {}", e);
        tracing::warn!("⚠️  Background journal processing disabled");
    } else {
        tracing::info!("🔄 Background journal processing started");
    }

    // Initialize prompt generator using the shared LLM manager
    let prompt_generator = {
        // Initialize prompt generator
        let prompt_generator = Arc::new(crate::prompt_generator::PromptGenerator::new(
            journal_manager.clone(),
            llm_manager.clone(),
            config.clone(),
            personalization_config.clone(),
        ));
        
        // Start the prompt generator service
        if let Err(e) = prompt_generator.start().await {
            tracing::error!("❌ Failed to start prompt generator: {}", e);
            None
        } else {
            tracing::info!("🎯 Prompt generator service started successfully");
            Some(prompt_generator)
        }
    };

    // Create shared application state
    let app_state = AppState {
        auth_manager: auth_manager.clone(),
        tokens_file_manager: tokens_file_manager.clone(),
        config: config.clone(),
        journal_manager: journal_manager.clone(),
        prompt_generator,
        personalization_config,
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