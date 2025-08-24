use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PromptVariations {
    pub second: String,
    pub third: String,
    pub additional: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PromptsConfig {
    pub summary_generation: String,
    pub daily_prompt: String,
    pub weekly_reflection: String,
    pub monthly_reflection: String,
    pub yearly_reflection: String,
    pub prompt_variations: PromptVariations,
}

impl Default for PromptsConfig {
    fn default() -> Self {
        Self {
            summary_generation: "Please summarize the following journal entry in 2-3 sentences, focusing on key emotions, events, and insights:\n\n{entry_content}\n\nSummary:".to_string(),
            daily_prompt: "Based on the following journal summaries from the past week, create an insightful and thought-provoking journal prompt for today. The prompt should help the person reflect on patterns, growth, or connections to recent experiences:\n\n{context}\n\nToday's journal prompt:".to_string(),
            weekly_reflection: "Based on the following journal entries from the past week, create a reflective prompt that encourages deeper weekly reflection on themes, patterns, growth, and lessons learned:\n\n{context}\n\nWeekly reflection prompt:".to_string(),
            monthly_reflection: "Based on the following weekly reflections from the past month, create a comprehensive monthly reflection prompt that explores broader patterns, achievements, challenges, and personal growth:\n\n{context}\n\nMonthly reflection prompt:".to_string(),
            yearly_reflection: "Based on the following monthly reflections from the past year, create a profound yearly reflection prompt that encourages deep introspection on personal transformation, major themes, life lessons, and future aspirations:\n\n{context}\n\nYearly reflection prompt:".to_string(),
            prompt_variations: PromptVariations {
                second: "\n\nCreate a different perspective or angle for this prompt:".to_string(),
                third: "\n\nCreate a third unique approach to this reflection:".to_string(),
                additional: "\n\nCreate another unique and creative approach to this reflection (variation #{number}):".to_string(),
            },
        }
    }
}

impl PromptsConfig {
    /// Load prompts configuration from file, create default if missing
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let path = path.as_ref();
        
        if !path.exists() {
            tracing::info!("Creating default prompts.json file");
            let default_config = Self::default();
            let json = serde_json::to_string_pretty(&default_config)?;
            fs::write(path, json)?;
            return Ok(default_config);
        }
        
        let content = fs::read_to_string(path)?;
        let config: PromptsConfig = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse prompts.json: {}", e))?;
        
        tracing::info!("Loaded prompts configuration from {}", path.display());
        Ok(config)
    }
    
    /// Get summary generation prompt with entry content substituted
    pub fn get_summary_prompt(&self, entry_content: &str) -> String {
        self.summary_generation.replace("{entry_content}", entry_content)
    }
    
    /// Get prompt template for the given prompt type with context substituted
    pub fn get_prompt_template(&self, prompt_type: &crate::journal::PromptType, context: &str) -> String {
        let template = match prompt_type {
            crate::journal::PromptType::Daily => &self.daily_prompt,
            crate::journal::PromptType::WeeklyReflection => &self.weekly_reflection,
            crate::journal::PromptType::MonthlyReflection => &self.monthly_reflection,
            crate::journal::PromptType::YearlyReflection => &self.yearly_reflection,
        };
        
        template.replace("{context}", context)
    }
    
    /// Get variation suffix for additional prompt numbers
    pub fn get_variation_suffix(&self, prompt_number: u8) -> String {
        match prompt_number {
            1 => String::new(), // No suffix for first prompt
            2 => self.prompt_variations.second.clone(),
            3 => self.prompt_variations.third.clone(),
            n if n > 3 => self.prompt_variations.additional.replace("{number}", &n.to_string()),
            _ => String::new(),
        }
    }
    
    /// Create example prompts.json file for user reference
    pub fn create_example<P: AsRef<Path>>(path: P) -> Result<(), Box<dyn std::error::Error>> {
        let example_path = path.as_ref().with_extension("example.json");
        let default_config = Self::default();
        let json = serde_json::to_string_pretty(&default_config)?;
        fs::write(&example_path, json)?;
        tracing::info!("Created example prompts file: {}", example_path.display());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_default_prompts() {
        let temp_file = NamedTempFile::new().unwrap();
        let config = PromptsConfig::load(temp_file.path()).unwrap();
        
        assert!(config.summary_generation.contains("Please summarize"));
        assert!(config.daily_prompt.contains("Today's journal prompt"));
    }

    #[test]
    fn test_prompt_substitution() {
        let config = PromptsConfig::default();
        let context = "Sample context";
        let prompt_type = crate::journal::PromptType::Daily;
        
        let result = config.get_prompt_template(&prompt_type, context);
        assert!(result.contains("Sample context"));
        assert!(!result.contains("{context}"));
    }

    #[test]
    fn test_variation_suffixes() {
        let config = PromptsConfig::default();
        
        assert_eq!(config.get_variation_suffix(1), "");
        assert!(config.get_variation_suffix(2).contains("different perspective"));
        assert!(config.get_variation_suffix(3).contains("third unique"));
        assert!(config.get_variation_suffix(5).contains("variation #5"));
    }
}
