use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Form, Json, Router,
};
use axum::body::Body;
use askama::Template;
use serde::{Deserialize, Serialize};

use crate::AppState;

#[derive(Deserialize)]
pub struct LoginForm {
    passcode: String,
    device_name: Option<String>,
    is_physical_device: Option<String>, // "true" or anything else for false
}

/// Templates for journal pages
#[derive(Template)]
#[template(path = "journal.html")]
pub struct JournalTemplate {
    pub cycle_date: String,
    pub real_date: String,
    pub real_date_iso: String,  // For the date picker (YYYY-MM-DD format)
    pub entry_type: String,
    pub existing_content: String,
    pub prompts: Vec<crate::journal::JournalPrompt>,
    pub is_today: bool,
    pub prev_date: String,
    pub next_date: String,
}

/// Form for journal entry submission
#[derive(Deserialize)]
pub struct JournalEntryForm {
    pub content: String,
}

/// Query parameters for journal date
#[derive(Deserialize)]
pub struct JournalDateQuery {
    pub date: Option<String>,
    pub gregorian_date: Option<String>,
}

/// Creates all routes - simple and clean
pub fn create_routes() -> Router<AppState> {
    use tower_http::services::ServeDir;
    Router::new()
        .route("/", get(journal_home_page))
        .route("/login", get(login_page).post(handle_login))
        .route("/logout", post(handle_logout))
        // Journal routes
        .route("/journal", get(journal_page))
        .route("/journal/entry", post(submit_journal_entry))
        .route("/journal/entry.json", get(get_journal_entry_json))
        .route("/journal/generate-prompt", post(generate_prompt_endpoint))
        .route("/journal/navigate-prompt", post(navigate_prompt_endpoint))
        .nest_service("/static", ServeDir::new("static"))
}

/// Home page - simple journal landing page
async fn journal_home_page(
    State(app_state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    // Extract token from cookie
    let token = extract_session_token(&headers);

    // Check if authenticated
    if let Some(token) = token {
        if app_state.auth_manager.validate_session(&token).await {
            let cycle_date = crate::cycle_date::CycleDate::today();
            let real_date = cycle_date.to_real_date().format("%A, %B %d, %Y").to_string();
            
            let html = format!(r#"
<!DOCTYPE html>
<html>
<head>
    <title>LLM Journal</title>
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <style>
        body {{ font-family: Arial, sans-serif; max-width: 800px; margin: 50px auto; padding: 20px; background: #f5f5f5; }}
        .container {{ background: white; padding: 30px; border-radius: 10px; box-shadow: 0 2px 10px rgba(0,0,0,0.1); }}
        h1 {{ color: #333; border-bottom: 2px solid #007acc; padding-bottom: 10px; }}
        .date-info {{ background: #e7f3ff; padding: 15px; border-radius: 5px; margin: 20px 0; }}
        .nav {{ margin: 20px 0; }}
        .nav a {{ display: inline-block; margin-right: 15px; padding: 10px 20px; background: #007acc; color: white; text-decoration: none; border-radius: 5px; }}
        .nav a:hover {{ background: #005a9e; }}
        .logout {{ float: right; background: #dc3545; }}
        .logout:hover {{ background: #c82333; }}
    </style>
</head>
<body>
    <div class="container">
        <h1>📝 LLM Journal</h1>
        <div class="date-info">
            <strong>Today:</strong> {}<br>
            <strong>Cycle Date:</strong> {}
        </div>
        <div class="nav">
            <a href="/journal">✍️ Write Entry</a>
            <a href="/journal/history">📚 View History</a>
            <form method="post" action="/logout" style="display: inline;">
                <button type="submit" class="nav logout">🚪 Logout</button>
            </form>
        </div>
        <p>Welcome to your LLM-powered journal! Choose an action above to get started.</p>
    </div>
</body>
</html>
            "#, real_date, cycle_date.to_string());
            
            return Html(html).into_response();
        }
    }

    // Not authenticated - redirect to login
    redirect_to_login().into_response()
}



/// Login page
async fn login_page(State(app_state): State<AppState>) -> Html<String> {
    // Generate passcode and show login form
    let _passcode = app_state.auth_manager.create_auth_request(None, false).await;
    
    let html = r#"
<!DOCTYPE html>
<html>
<head>
    <title>LLM Journal - Login</title>
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <style>
        body { font-family: Arial, sans-serif; max-width: 400px; margin: 100px auto; padding: 20px; background: #f0f0f0; }
        .login-box { background: white; padding: 30px; border-radius: 10px; box-shadow: 0 2px 10px rgba(0,0,0,0.1); }
        input[type="text"], input[type="password"] { width: 100%; padding: 12px; margin: 10px 0; border: 1px solid #ddd; border-radius: 5px; box-sizing: border-box; }
        button { width: 100%; padding: 12px; background: #007acc; color: white; border: none; border-radius: 5px; cursor: pointer; font-size: 16px; }
        button:hover { background: #005a9e; }
        .info { background: #e7f3ff; padding: 15px; border-radius: 5px; margin-bottom: 20px; border-left: 4px solid #007acc; }
    </style>
</head>
<body>
    <div class="login-box">
        <h2>📝 LLM Journal</h2>
        <div class="info">
            <strong>Device Authentication</strong><br>
            Check the server terminal for your unique passcode.
        </div>
        <form method="post" action="/login">
            <input type="text" name="device_name" placeholder="Device name (optional)" maxlength="50">
            <input type="password" name="passcode" placeholder="Enter passcode from terminal" required autofocus>
            <label style="display: flex; align-items: center; margin: 10px 0; cursor: pointer;">
                <input type="checkbox" name="is_physical_device" value="true" style="margin-right: 8px;">
                This is a custom device with physical button
            </label>
            <button type="submit">Authenticate</button>
        </form>
        <p><small>Passcode expires in 10 minutes.</small></p>
    </div>
</body>
</html>
    "#.to_string();
    
    Html(html)
}

/// Handle login submission
async fn handle_login(
    State(app_state): State<AppState>,
    Form(form): Form<LoginForm>,
) -> Response {
    let is_physical_device = form.is_physical_device.as_deref() == Some("true");
    
    if let Some(token) = app_state.auth_manager.authenticate(&form.passcode, form.device_name, is_physical_device).await {
        // Save session immediately
        app_state.auth_manager.save_sessions_to_file(&app_state.tokens_file_manager).await;
        
        // Use the configured session duration from config
        let max_age = app_state.config.auth.session_duration_seconds;
        let cookie = format!("session_token={}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}", token, max_age);
        
        (
            StatusCode::OK,
            [("Set-Cookie", cookie.as_str())],
            Redirect::to("/"),            
        ).into_response()
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Html(r#"
<!DOCTYPE html>
<html>
<head><title>Login Failed</title><meta http-equiv="refresh" content="3;url=/login"></head>
<body><h2>❌ Invalid Passcode</h2><p>Redirecting...</p></body>
</html>
            "#),
        ).into_response()
    }
}

/// Handle logout
async fn handle_logout(
    State(app_state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if let Some(token) = extract_session_token(&headers) {
        app_state.auth_manager.remove_session(&token).await;
        app_state.auth_manager.save_sessions_to_file(&app_state.tokens_file_manager).await;
    }
    
    // Clear cookie and redirect (303 forces GET request)
    (
        StatusCode::SEE_OTHER,
        [
            ("Location", "/login"),
            ("Set-Cookie", "session_token=; Path=/; HttpOnly; Max-Age=0"),
        ],
        Html("Logged out"),
    ).into_response()
}

/// Extract session token from request headers
fn extract_session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::COOKIE)
        .and_then(|cookie| cookie.to_str().ok())
        .and_then(|cookie_str| {
            cookie_str
                .split(';')
                .find(|part| part.trim().starts_with("session_token="))
                .map(|part| part.trim().strip_prefix("session_token=").unwrap_or("").to_string())
        })
}

// Journal-specific handlers
/// Journal page - shows today's prompt and entry form
async fn journal_page(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<JournalDateQuery>,
) -> Response {
    // Extract token from cookie
    let token = extract_session_token(&headers);

    // Check if authenticated
    if let Some(token) = token {
        if app_state.auth_manager.validate_session(&token).await {
            // Determine which date to show
            let cycle_date = if let Some(gregorian_date_str) = params.gregorian_date {
                // Convert Gregorian date to cycle date
                match chrono::NaiveDate::parse_from_str(&gregorian_date_str, "%Y-%m-%d") {
                    Ok(gregorian_date) => crate::cycle_date::CycleDate::from_real_date(gregorian_date),
                    Err(_) => {
                        tracing::warn!("Invalid gregorian date format: {}", gregorian_date_str);
                        crate::cycle_date::CycleDate::today()
                    }
                }
            } else if let Some(date_str) = params.date {
                // Use cycle date directly
                match crate::cycle_date::CycleDate::from_string(&date_str) {
                    Ok(date) => date,
                    Err(_) => crate::cycle_date::CycleDate::today(),
                }
            } else {
                crate::cycle_date::CycleDate::today()
            };

            // Use shared journal manager
            let journal_manager = &app_state.journal_manager;

            // Load existing entry if it exists
            let existing_entry = match journal_manager.load_entry(&cycle_date).await {
                Ok(entry) => entry,
                Err(e) => {
                    tracing::error!("Failed to load journal entry: {}", e);
                    None
                }
            };

            // Load prompts for this date
            let mut prompts = Vec::new();
            for i in 1..=app_state.config.journal.max_prompts_per_day {
                if let Ok(Some(prompt)) = journal_manager.load_prompt(&cycle_date, i).await {
                    prompts.push(prompt);
                }
            }

            // Determine entry type based on cycle date pattern
            let cycle_str = cycle_date.to_string();
            let entry_type = if cycle_str.ends_with("000") {
                "Yearly Reflection"
            } else if cycle_str.ends_with("00") {
                "Monthly Reflection"
            } else if cycle_str.ends_with("0") {
                "Weekly Reflection"
            } else {
                "Daily Entry"
            };

            let template = JournalTemplate {
                cycle_date: cycle_date.to_string(),
                real_date: cycle_date.to_real_date().format("%A, %B %d, %Y").to_string(),
                real_date_iso: cycle_date.to_real_date().format("%Y-%m-%d").to_string(),
                entry_type: entry_type.to_string(),
                existing_content: existing_entry.map(|e| e.content).unwrap_or_default(),
                prompts,
                is_today: cycle_date == crate::cycle_date::CycleDate::today(),
                prev_date: cycle_date.previous_day().to_string(),
                next_date: cycle_date.next_day().to_string(),
            };

            return match template.render() {
                Ok(html) => Html(html).into_response(),
                Err(e) => {
                    tracing::error!("Failed to render journal template: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, Html("Error rendering page")).into_response()
                }
            };
        }
    }

    // Not authenticated - redirect to login
    redirect_to_login().into_response()
}

/// Handle journal entry submission
async fn submit_journal_entry(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<JournalEntryForm>,
) -> Response {
    // Extract token from cookie
    let token = extract_session_token(&headers);

    // Check if authenticated
    if let Some(token) = token {
        if app_state.auth_manager.validate_session(&token).await {
            let cycle_date = crate::cycle_date::CycleDate::today();
            let journal_manager = &app_state.journal_manager;

            let entry = crate::journal::JournalEntry {
                cycle_date,
                content: form.content,
                created_at: chrono::Local::now(),
                modified_at: chrono::Local::now(),
            };

            match journal_manager.save_entry(&entry).await {
                Ok(()) => {
                    tracing::info!("Journal entry saved for {}", entry.cycle_date);
                    // Redirect back to journal page
                    return (
                        StatusCode::SEE_OTHER,
                        [("Location", "/journal")],
                        Html("Entry saved successfully"),
                    ).into_response();
                }
                Err(e) => {
                    tracing::error!("Failed to save journal entry: {}", e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, Html("Error saving entry")).into_response();
                }
            }
        }
    }

    // Not authenticated - redirect to login
    redirect_to_login().into_response()
}

/// Get journal entry as JSON (for auto-save functionality)
async fn get_journal_entry_json(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<JournalDateQuery>,
) -> Response {
    // Extract token from cookie
    let token = extract_session_token(&headers);

    // Check if authenticated
    if let Some(token) = token {
        if app_state.auth_manager.validate_session(&token).await {
            let cycle_date = if let Some(date_str) = params.date {
                match crate::cycle_date::CycleDate::from_string(&date_str) {
                    Ok(date) => date,
                    Err(_) => crate::cycle_date::CycleDate::today(),
                }
            } else {
                crate::cycle_date::CycleDate::today()
            };

            let journal_manager = &app_state.journal_manager;
            
            match journal_manager.load_entry(&cycle_date).await {
                Ok(Some(entry)) => {
                    match serde_json::to_string(&entry) {
                        Ok(json) => {
                            return Response::builder()
                                .header("Content-Type", "application/json")
                                .body(json.into())
                                .unwrap();
                        }
                        Err(e) => {
                            tracing::error!("Failed to serialize entry: {}", e);
                            return (StatusCode::INTERNAL_SERVER_ERROR, "Error serializing entry").into_response();
                        }
                    }
                }
                Ok(None) => {
                    return Response::builder()
                        .header("Content-Type", "application/json")
                        .body("null".into())
                        .unwrap();
                }
                Err(e) => {
                    tracing::error!("Failed to load entry: {}", e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Error loading entry").into_response();
                }
            }
        }
    }

    (StatusCode::UNAUTHORIZED, "Unauthorized").into_response()
}

/// Form for prompt generation request
#[derive(Deserialize)]
pub struct GeneratePromptForm {
    pub entry_type: String,
    pub cycle_date: String,
}

/// Response for prompt generation
#[derive(serde::Serialize)]
pub struct GeneratePromptResponse {
    pub prompt: String,
}

/// Generate LLM prompt endpoint
async fn generate_prompt_endpoint(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Json(form): Json<GeneratePromptForm>,
) -> Response {
    // Extract token from cookie
    let token = extract_session_token(&headers);

    // Check if authenticated
    if let Some(token) = token {
        if app_state.auth_manager.validate_session(&token).await {
            tracing::info!("🤖 Generating prompt for entry type: {}", form.entry_type);
            
            // Parse cycle date
            let cycle_date = match crate::cycle_date::CycleDate::from_string(&form.cycle_date) {
                Ok(date) => date,
                Err(e) => {
                    tracing::error!("Invalid cycle date: {}", e);
                    return (StatusCode::BAD_REQUEST, "Invalid cycle date").into_response();
                }
            };

            // Create LLM worker (this will be moved to app state in the future)
            let model_path = app_state.config.llm.model_path.clone();
            
            let llm_worker = match crate::llm_worker::LlmWorker::new(
                model_path, 
                app_state.config.llm.temperature, 
                app_state.config.llm.max_tokens
            ) {
                Ok(worker) => worker,
                Err(e) => {
                    tracing::error!("Failed to create LLM worker: {}", e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "LLM initialization failed").into_response();
                }
            };

            // Load model if not already loaded
            if let Err(e) = llm_worker.load_model().await {
                tracing::error!("Failed to load LLM model: {}", e);
                return (StatusCode::INTERNAL_SERVER_ERROR, "Model loading failed").into_response();
            }

            // Create prompt based on entry type
            let prompt_request = match form.entry_type.as_str() {
                "Daily Entry" => "Create a thoughtful journal prompt for daily reflection",
                "Weekly Reflection" => "Create a journal prompt for weekly reflection and growth",
                "Monthly Reflection" => "Create a journal prompt for monthly introspection and goal assessment",
                "Yearly Reflection" => "Create a journal prompt for deep yearly reflection and life review",
                _ => "Create a meaningful journal prompt for personal reflection",
            };

            // Generate the prompt
            match llm_worker.generate_text(prompt_request, 200).await {
                Ok(generated_prompt) => {
                    let response = GeneratePromptResponse {
                        prompt: generated_prompt,
                    };
                    
                    match serde_json::to_string(&response) {
                        Ok(json) => {
                            return Response::builder()
                                .header("Content-Type", "application/json")
                                .body(json.into())
                                .unwrap();
                        }
                        Err(e) => {
                            tracing::error!("Failed to serialize prompt response: {}", e);
                            return (StatusCode::INTERNAL_SERVER_ERROR, "Serialization error").into_response();
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to generate prompt: {}", e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Prompt generation failed").into_response();
                }
            }
        }
    }

    (StatusCode::UNAUTHORIZED, "Unauthorized").into_response()
}

/// Form for prompt navigation request
#[derive(Deserialize)]
pub struct PromptNavigationForm {
    pub cycle_date: String,
    pub current_prompt: u32,
    pub direction: String, // "next" or "prev"
}

/// Response for prompt navigation
#[derive(serde::Serialize)]
pub struct PromptNavigationResponse {
    pub prompt: Option<String>,
    pub prompt_number: u32,
    pub prompt_type: String,
    pub has_prev: bool,
    pub has_next: bool,
    pub generated_new: bool,
}

/// Navigate between prompts (next/previous)
async fn navigate_prompt_endpoint(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Json(form): Json<PromptNavigationForm>,
) -> Response {
    // Extract token from cookie
    let token = extract_session_token(&headers);

    // Check if authenticated
    if let Some(token) = token {
        if app_state.auth_manager.validate_session(&token).await {
            tracing::info!("🔄 Navigation request: current_prompt={}, direction={}, cycle_date={}", 
                form.current_prompt, form.direction, form.cycle_date);
            
            // Calculate new prompt number based on direction
            let new_prompt_number = match form.direction.as_str() {
                "next" => form.current_prompt + 1,
                "prev" => {
                    if form.current_prompt > 1 {
                        form.current_prompt - 1
                    } else {
                        1
                    }
                }
                _ => {
                    return (StatusCode::BAD_REQUEST, "Invalid direction").into_response();
                }
            };

            let response = PromptNavigationResponse {
                prompt: Some(format!("Generated prompt #{} for {}", new_prompt_number, form.cycle_date)),
                prompt_number: new_prompt_number,
                prompt_type: "Daily".to_string(),
                has_prev: new_prompt_number > 1,
                has_next: true,
                generated_new: true,
            };
            
            match serde_json::to_string(&response) {
                Ok(json) => {
                    return Response::builder()
                        .header("Content-Type", "application/json")
                        .body(json.into())
                        .unwrap();
                }
                Err(e) => {
                    tracing::error!("Failed to serialize navigation response: {}", e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Serialization error").into_response();
                }
            }
        }
    }

    (StatusCode::UNAUTHORIZED, "Unauthorized").into_response()
}

/// Redirect to login page
fn redirect_to_login() -> (StatusCode, [(&'static str, &'static str); 1], Html<&'static str>) {
    (
        StatusCode::TEMPORARY_REDIRECT,
        [("Location", "/login")],
        Html("Redirecting to login..."),
    )
}
