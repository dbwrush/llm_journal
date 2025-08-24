use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use crate::prompts::PromptsConfig;
use chrono::{NaiveDate, Local, Datelike};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Holiday {
    pub name: String,
    pub date: String, // Format: "MM-DD" for recurring annual events or "YYYY-MM-DD" for specific dates
    pub category: String, // "birthday", "anniversary", "religious", "cultural", "personal", "seasonal", "work"
    pub description: Option<String>,
    pub recurring: bool, // true for annual events like birthdays
}

/// Complete personalization configuration combining all user customization files
#[derive(Debug, Clone)]
pub struct PersonalizationConfig {
    pub prompts: PromptsConfig,
    pub profile: Option<String>,
    pub style: Option<String>,
    pub status: Option<String>,
    pub holidays: Vec<Holiday>,
    journal_dir: PathBuf,
}

impl PersonalizationConfig {
    /// Load complete personalization configuration from the journal directory
    pub fn load<P: AsRef<Path>>(journal_dir: P) -> Result<Self, Box<dyn std::error::Error>> {
        let journal_dir = journal_dir.as_ref();
        
        // Load prompts.json
        let prompts_path = journal_dir.join("prompts.json");
        let prompts = PromptsConfig::load(&prompts_path)?;
        
        // Load profile.txt (static user context)
        let profile_path = journal_dir.join("profile.txt");
        let profile = Self::load_text_file(&profile_path, "profile.txt", Self::default_profile_content())?;
        
        // Load style.txt (AI personality configuration)
        let style_path = journal_dir.join("style.txt");
        let style = Self::load_text_file(&style_path, "style.txt", Self::default_style_content())?;
        
        // Load status.txt (dynamic user context, may not exist initially)
        let status_path = journal_dir.join("status.txt");
        let status = Self::load_text_file_optional(&status_path, "status.txt")?;
        
        // Load holidays.txt (temporal context)
        let holidays_path = journal_dir.join("holidays.txt");
        let holidays = Self::load_holidays(&holidays_path)?;

        Ok(Self {
            prompts,
            profile,
            style,
            status,
            holidays,
            journal_dir: journal_dir.to_path_buf(),
        })
    }
    
    /// Load a text file, creating it with default content if it doesn't exist
    fn load_text_file<P: AsRef<Path>>(
        path: P, 
        filename: &str, 
        default_content: String
    ) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let path = path.as_ref();
        
        if !path.exists() {
            tracing::info!("Creating default {} file", filename);
            fs::write(path, &default_content)?;
            return Ok(Some(default_content));
        }
        
        match fs::read_to_string(path) {
            Ok(content) => {
                let trimmed = content.trim();
                if trimmed.is_empty() {
                    tracing::warn!("{} is empty, using default content", filename);
                    Ok(Some(default_content))
                } else {
                    tracing::info!("Loaded {} ({} characters)", filename, trimmed.len());
                    Ok(Some(trimmed.to_string()))
                }
            }
            Err(e) => {
                tracing::error!("Failed to read {}: {}", filename, e);
                tracing::info!("Using default {} content", filename);
                Ok(Some(default_content))
            }
        }
    }
    
    /// Load a text file that may not exist (like status.txt)
    fn load_text_file_optional<P: AsRef<Path>>(
        path: P, 
        filename: &str
    ) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let path = path.as_ref();
        
        if !path.exists() {
            tracing::info!("{} does not exist yet (will be created during summary generation)", filename);
            return Ok(None);
        }
        
        match fs::read_to_string(path) {
            Ok(content) => {
                let trimmed = content.trim();
                if trimmed.is_empty() {
                    tracing::info!("{} is empty", filename);
                    Ok(None)
                } else {
                    tracing::info!("Loaded {} ({} characters)", filename, trimmed.len());
                    Ok(Some(trimmed.to_string()))
                }
            }
            Err(e) => {
                tracing::error!("Failed to read {}: {}", filename, e);
                Ok(None)
            }
        }
    }
    
    /// Load holidays.txt and parse into Holiday structs
    fn load_holidays<P: AsRef<Path>>(
        path: P
    ) -> Result<Vec<Holiday>, Box<dyn std::error::Error>> {
        let path = path.as_ref();
        
        if !path.exists() {
            tracing::info!("holidays.txt does not exist, creating with default content");
            let default_content = Self::default_holidays_content();
            fs::write(path, &default_content)?;
            return Self::parse_holidays(&default_content);
        }
        
        match fs::read_to_string(path) {
            Ok(content) => {
                tracing::info!("Loaded holidays.txt ({} characters)", content.len());
                Self::parse_holidays(&content)
            }
            Err(e) => {
                tracing::error!("Failed to read holidays.txt: {}", e);
                tracing::info!("Using default holidays content");
                let default_content = Self::default_holidays_content();
                Self::parse_holidays(&default_content)
            }
        }
    }
    
    /// Parse holidays content into Holiday structs
    fn parse_holidays(content: &str) -> Result<Vec<Holiday>, Box<dyn std::error::Error>> {
        let mut holidays = Vec::new();
        
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            
            // Parse format: "DATE|CATEGORY|NAME|DESCRIPTION" or "DATE|CATEGORY|NAME"
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() >= 3 {
                let date = parts[0].trim().to_string();
                let category = parts[1].trim().to_string();
                let name = parts[2].trim().to_string();
                let description = if parts.len() > 3 && !parts[3].trim().is_empty() {
                    Some(parts[3].trim().to_string())
                } else {
                    None
                };
                
                // Determine if recurring based on date format
                let recurring = !date.contains('-') || date.len() == 5; // MM-DD format or just MM
                
                holidays.push(Holiday {
                    name,
                    date,
                    category,
                    description,
                    recurring,
                });
            }
        }
        
        tracing::info!("Parsed {} holidays from holidays.txt", holidays.len());
        Ok(holidays)
    }
    
    /// Get enriched context by combining journal context with personalization
    pub fn enrich_context(&self, base_context: &str) -> String {
        let mut enriched = String::new();
        
        // Add temporal context (current date and upcoming events)
        enriched.push_str(&self.get_temporal_context());
        
        // Add user profile context
        if let Some(profile) = &self.profile {
            if !profile.trim().is_empty() {
                enriched.push_str("USER PROFILE:\n");
                enriched.push_str(profile);
                enriched.push_str("\n\n");
            }
        }
        
        // Add AI style instructions
        if let Some(style) = &self.style {
            if !style.trim().is_empty() {
                enriched.push_str("COMMUNICATION STYLE:\n");
                enriched.push_str(style);
                enriched.push_str("\n\n");
            }
        }
        
        // Add dynamic status context
        if let Some(status) = &self.status {
            if !status.trim().is_empty() {
                enriched.push_str("CURRENT STATUS:\n");
                enriched.push_str(status);
                enriched.push_str("\n\n");
            }
        }
        
        // Add the base journal context
        enriched.push_str("JOURNAL CONTEXT:\n");
        enriched.push_str(base_context);
        
        enriched
    }
    
    /// Update the status.txt file with new context from LLM
    pub fn update_status(&mut self, new_status: String) -> Result<(), Box<dyn std::error::Error>> {
        let status_path = self.journal_dir.join("status.txt");
        
        // Write the new status to file
        fs::write(&status_path, &new_status)?;
        
        // Update the in-memory status
        self.status = Some(new_status);
        
        tracing::info!("Updated status.txt with new context");
        Ok(())
    }
    
    /// Get the current status for the LLM to reference when updating
    pub fn get_current_status(&self) -> Option<&String> {
        self.status.as_ref()
    }
    
    /// Get upcoming holidays within the next 30 days
    pub fn get_upcoming_holidays(&self) -> Vec<&Holiday> {
        let today = Local::now().date_naive();
        let mut upcoming = Vec::new();
        
        for holiday in &self.holidays {
            if let Some(days_until) = self.days_until_holiday(holiday, today) {
                if days_until <= 30 {
                    upcoming.push(holiday);
                }
            }
        }
        
        // Sort by days until holiday
        upcoming.sort_by_key(|h| self.days_until_holiday(h, today).unwrap_or(365));
        upcoming
    }
    
    /// Calculate days until a holiday from the given date
    fn days_until_holiday(&self, holiday: &Holiday, from_date: NaiveDate) -> Option<i64> {
        let current_year = from_date.year();
        
        // Parse the holiday date
        if holiday.date.len() == 5 && holiday.date.contains('-') {
            // MM-DD format (recurring annual)
            let parts: Vec<&str> = holiday.date.split('-').collect();
            if parts.len() == 2 {
                if let (Ok(month), Ok(day)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
                    // Try this year first
                    if let Some(this_year_date) = NaiveDate::from_ymd_opt(current_year, month, day) {
                        let days_diff = (this_year_date - from_date).num_days();
                        if days_diff >= 0 {
                            return Some(days_diff);
                        }
                    }
                    
                    // If this year's date has passed, try next year
                    if let Some(next_year_date) = NaiveDate::from_ymd_opt(current_year + 1, month, day) {
                        return Some((next_year_date - from_date).num_days());
                    }
                }
            }
        } else if holiday.date.len() == 10 && holiday.date.matches('-').count() == 2 {
            // YYYY-MM-DD format (specific date)
            if let Ok(specific_date) = NaiveDate::parse_from_str(&holiday.date, "%Y-%m-%d") {
                let days_diff = (specific_date - from_date).num_days();
                if days_diff >= 0 {
                    return Some(days_diff);
                }
            }
        }
        
        None
    }
    
    /// Get temporal context for the current date
    pub fn get_temporal_context(&self) -> String {
        let today = Local::now();
        let date_str = today.format("%A, %B %d, %Y").to_string();
        let upcoming_holidays = self.get_upcoming_holidays();
        
        let mut context = format!("CURRENT DATE: {}\n\n", date_str);
        
        if !upcoming_holidays.is_empty() {
            context.push_str("UPCOMING EVENTS (next 30 days):\n");
            for holiday in upcoming_holidays.iter().take(5) { // Limit to 5 most relevant
                if let Some(days) = self.days_until_holiday(holiday, today.date_naive()) {
                    let day_text = if days == 0 {
                        "TODAY".to_string()
                    } else if days == 1 {
                        "tomorrow".to_string()
                    } else {
                        format!("in {} days", days)
                    };
                    
                    context.push_str(&format!(
                        "- {} ({}): {} {}\n", 
                        holiday.name,
                        day_text,
                        holiday.category,
                        holiday.description.as_ref().map(|d| format!("- {}", d)).unwrap_or_default()
                    ));
                }
            }
            context.push('\n');
        }
        
        context
    }
    
    /// Default content for profile.txt
    fn default_profile_content() -> String {
        r#"This file contains static information about you that will be included as context in all journal prompts.

Edit this file to include personal details that will help the AI generate more relevant and personalized prompts. This context will be included in every prompt generation, so keep it focused on information that's consistently relevant to your journaling.

Examples of what to include:
- Your general life situation (occupation, family status, living situation)
- Core values and beliefs that influence your daily life
- Long-term goals or projects you're working on
- Significant relationships and their importance to you
- Personal interests, hobbies, or passions
- Health conditions or lifestyle factors that affect your daily experience
- Spiritual or philosophical practices you engage in

Keep this information current but avoid including temporary situations that change frequently - those belong in status.txt instead.

Example profile:
---
I'm a software developer in my early 30s, living in Seattle with my partner and our dog. I value work-life balance and am passionate about sustainable living. Currently working on launching a side project while maintaining my full-time job. I practice meditation daily and enjoy hiking on weekends. Building stronger connections with family is important to me this year."#.to_string()
    }
    
    /// Default content for style.txt
    fn default_style_content() -> String {
        r#"This file defines how the AI should communicate when generating journal prompts and responses.

Edit this file to customize the AI's personality, tone, and approach to match your preferences. This will influence how all prompts are written and the overall feeling of your journaling experience.

Examples of what to include:
- Preferred communication tone (formal, casual, encouraging, direct, etc.)
- Specific words or phrases you like or dislike
- Cultural or philosophical perspective you want reflected
- Level of challenge vs. comfort in prompts
- Preferred prompt length and structure
- Any specific therapeutic or self-development approaches you prefer

Example style guide:
---
Please communicate in a warm, encouraging tone that balances gentle support with thoughtful challenge. I prefer prompts that are introspective but not overly serious - include moments of lightness and curiosity. Use language that feels like a wise, supportive friend rather than a clinical therapist. Keep prompts concise but meaningful, typically 2-3 sentences. I appreciate metaphors from nature and gentle humor when appropriate. Avoid overly abstract language and focus on practical, actionable reflection."#.to_string()
    }
    
    /// Default content for holidays.txt
    fn default_holidays_content() -> String {
        r#"# This file contains important dates that will influence your journal prompts
# Format: DATE|CATEGORY|NAME|DESCRIPTION (description is optional)
# DATE formats:
#   - MM-DD for recurring annual events (e.g., 12-25 for Christmas)
#   - YYYY-MM-DD for specific one-time dates
#   - MM for monthly recurring (e.g., first Monday, seasonal changes)"#.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[test]
    fn test_load_creates_default_files() {
        let temp_dir = TempDir::new().unwrap();
        let config = PersonalizationConfig::load(temp_dir.path()).unwrap();
        
        assert!(config.profile.is_some());
        assert!(config.style.is_some());
        
        // Verify files were created
        assert!(temp_dir.path().join("profile.txt").exists());
        assert!(temp_dir.path().join("style.txt").exists());
        assert!(temp_dir.path().join("prompts.json").exists());
    }
    
    #[test]
    fn test_enrich_context() {
        let config = PersonalizationConfig {
            prompts: PromptsConfig::default(),
            profile: Some("I'm a software developer".to_string()),
            style: Some("Be encouraging and direct".to_string()),
            status: Some("Currently working on a challenging project".to_string()),
            holidays: vec![], // Empty holidays for test
            journal_dir: PathBuf::from("/tmp"),
        };
        
        let base_context = "Recent journal entries show stress about work";
        let enriched = config.enrich_context(base_context);
        
        assert!(enriched.contains("USER PROFILE:"));
        assert!(enriched.contains("COMMUNICATION STYLE:"));
        assert!(enriched.contains("CURRENT STATUS:"));
        assert!(enriched.contains("JOURNAL CONTEXT:"));
        assert!(enriched.contains("software developer"));
        assert!(enriched.contains("encouraging and direct"));
        assert!(enriched.contains("challenging project"));
        assert!(enriched.contains("stress about work"));
    }
    
    #[test]
    fn test_temporal_awareness() {
        // Create a PersonalizationConfig with some test holidays
        let test_holidays = vec![
            Holiday {
                name: "Test Birthday".to_string(),
                date: "12-25".to_string(), // Christmas - recurring
                category: "birthday".to_string(),
                description: Some("Test person's birthday".to_string()),
                recurring: true,
            },
            Holiday {
                name: "New Year".to_string(),
                date: "01-01".to_string(),
                category: "holiday".to_string(),
                description: None,
                recurring: true,
            },
        ];
        
        let config = PersonalizationConfig {
            prompts: PromptsConfig::default(),
            profile: Some("Test user".to_string()),
            style: Some("Test style".to_string()),
            status: Some("Test status".to_string()),
            holidays: test_holidays,
            journal_dir: PathBuf::from("/tmp"),
        };
        
        // Test temporal context generation
        let temporal_context = config.get_temporal_context();
        assert!(temporal_context.contains("CURRENT DATE:"));
        
        // Test upcoming holidays detection
        let upcoming = config.get_upcoming_holidays();
        assert!(upcoming.len() <= 2); // Should not be more than our test holidays
        
        // Test enriched context includes temporal information
        let base_context = "Test journal context";
        let enriched = config.enrich_context(base_context);
        assert!(enriched.contains("CURRENT DATE:"));
        assert!(enriched.contains("USER PROFILE:"));
        assert!(enriched.contains("JOURNAL CONTEXT:"));
        
        println!("Temporal context test passed!");
        println!("Generated temporal context: {}", temporal_context);
    }
    
    #[test]
    fn test_real_holidays_functionality() {
        // Test loading the actual holidays.txt file if it exists
        let journal_dir = PathBuf::from("journal");
        if journal_dir.exists() {
            match PersonalizationConfig::load(&journal_dir) {
                Ok(config) => {
                    println!("\n=== REAL HOLIDAYS TEST ===");
                    
                    // Show all holidays
                    println!("Total holidays loaded: {}", config.holidays.len());
                    for (i, holiday) in config.holidays.iter().enumerate() {
                        println!("{}. {} ({}) - {} [{}]", 
                                 i + 1, 
                                 holiday.name, 
                                 holiday.date, 
                                 holiday.category,
                                 if holiday.recurring { "recurring" } else { "one-time" });
                    }
                    
                    // Show upcoming holidays
                    let upcoming = config.get_upcoming_holidays();
                    println!("\nUpcoming holidays in next 30 days: {}", upcoming.len());
                    for holiday in upcoming.iter().take(5) {
                        if let Some(days) = config.days_until_holiday(holiday, Local::now().date_naive()) {
                            let day_text = if days == 0 {
                                "TODAY".to_string()
                            } else if days == 1 {
                                "tomorrow".to_string()
                            } else {
                                format!("in {} days", days)
                            };
                            println!("- {} ({}): {} [{}]", 
                                     holiday.name, 
                                     day_text, 
                                     holiday.category,
                                     holiday.date);
                        }
                    }
                    
                    // Show full temporal context
                    let temporal_context = config.get_temporal_context();
                    println!("\n=== FULL TEMPORAL CONTEXT ===");
                    println!("{}", temporal_context);
                    
                    // Test enriched context
                    let enriched = config.enrich_context("User seems excited about upcoming holidays and seasonal changes.");
                    println!("\n=== ENRICHED CONTEXT SAMPLE ===");
                    println!("{}", enriched);
                    
                    println!("\nReal holidays test completed successfully!");
                } Err(e) => {
                    println!("Could not load real config (expected in tests): {}", e);
                }
            }
        } else {
            println!("Journal directory doesn't exist - this is expected in isolated tests");
        }
    }
}
