use crate::cycle_date::CycleDate;
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;

/// Represents a journal entry for a specific day
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub cycle_date: CycleDate,
    pub content: String,
    pub created_at: DateTime<Local>,
    pub modified_at: DateTime<Local>,
}

/// Represents a generated summary of a journal entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalSummary {
    pub cycle_date: CycleDate,
    pub summary: String,
    pub generated_at: DateTime<Local>,
}

/// Represents a writing sample uploaded by the user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WritingSample {
    pub cycle_date: CycleDate,
    pub sample_number: u8,
    pub content: String,
    pub uploaded_at: DateTime<Local>,
}

/// Represents a generated summary of a writing sample
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WritingSampleSummary {
    pub cycle_date: CycleDate,
    pub sample_number: u8,
    pub summary: String,
    pub generated_at: DateTime<Local>,
}

/// Represents a generated prompt for a specific day
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalPrompt {
    pub cycle_date: CycleDate,
    pub prompt: String,
    pub prompt_number: u8, // 1, 2, or 3 for multiple prompts per day
    pub generated_at: DateTime<Local>,
    pub prompt_type: PromptType,
}

/// Types of prompts that can be generated
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PromptType {
    Daily,          // Based on summaries from past 7 days
    WeeklyReflection,   // Based on full entries from past 7 days
    MonthlyReflection,  // Based on weekly reflections from past month
    YearlyReflection,   // Based on monthly reflections from past year
}

impl std::fmt::Display for PromptType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PromptType::Daily => write!(f, "Daily"),
            PromptType::WeeklyReflection => write!(f, "Weekly Reflection"),
            PromptType::MonthlyReflection => write!(f, "Monthly Reflection"),
            PromptType::YearlyReflection => write!(f, "Yearly Reflection"),
        }
    }
}

/// Manages journal files and operations
pub struct JournalManager {
    base_path: PathBuf,
}

impl JournalManager {
    pub fn new<P: AsRef<Path>>(base_path: P) -> Self {
        Self {
            base_path: base_path.as_ref().to_path_buf(),
        }
    }

    /// Create directory structure if it doesn't exist
    pub async fn ensure_directories(&self) -> Result<(), Box<dyn std::error::Error>> {
        fs::create_dir_all(&self.base_path).await?;
        Ok(())
    }

    /// Ensure that the directory for a specific date exists
    pub async fn ensure_date_directory(&self, cycle_date: &CycleDate) -> Result<(), Box<dyn std::error::Error>> {
        let date_dir = self.base_path.join(cycle_date.to_string());
        fs::create_dir_all(&date_dir).await?;
        Ok(())
    }

    /// Get file paths for a given cycle date
    pub fn get_file_paths(&self, cycle_date: &CycleDate) -> JournalFilePaths {
        let date_str = cycle_date.to_string();
        let date_dir = self.base_path.join(&date_str);
        JournalFilePaths {
            entry: date_dir.join("entry.txt"),
            summary: date_dir.join("summary.txt"),
            status: date_dir.join("status.txt"),
            prompt1: date_dir.join("prompt1.txt"),
            prompt2: date_dir.join("prompt2.txt"),
            prompt3: date_dir.join("prompt3.txt"),
            date_dir,
        }
    }

    /// Save a journal entry
    pub async fn save_entry(&self, entry: &JournalEntry) -> Result<(), Box<dyn std::error::Error>> {
        self.ensure_date_directory(&entry.cycle_date).await?;
        let paths = self.get_file_paths(&entry.cycle_date);
        
        let mut file = fs::File::create(&paths.entry).await?;
        file.write_all(entry.content.as_bytes()).await?;
        
        Ok(())
    }

    /// Load a journal entry
    pub async fn load_entry(&self, cycle_date: &CycleDate) -> Result<Option<JournalEntry>, Box<dyn std::error::Error>> {
        let paths = self.get_file_paths(cycle_date);
        
        if !paths.entry.exists() {
            return Ok(None);
        }
        
        let content = fs::read_to_string(&paths.entry).await?;
        let metadata = fs::metadata(&paths.entry).await?;
        
        let created_at = DateTime::from(metadata.created()?);
        let modified_at = DateTime::from(metadata.modified()?);
        
        Ok(Some(JournalEntry {
            cycle_date: *cycle_date,
            content,
            created_at,
            modified_at,
        }))
    }

    /// Save a journal summary
    pub async fn save_summary(&self, summary: &JournalSummary) -> Result<(), Box<dyn std::error::Error>> {
        self.ensure_directories().await?;
        let paths = self.get_file_paths(&summary.cycle_date);
        
        let mut file = fs::File::create(&paths.summary).await?;
        file.write_all(summary.summary.as_bytes()).await?;
        
        Ok(())
    }

    /// Load a journal summary
    pub async fn load_summary(&self, cycle_date: &CycleDate) -> Result<Option<JournalSummary>, Box<dyn std::error::Error>> {
        let paths = self.get_file_paths(cycle_date);
        
        if !paths.summary.exists() {
            return Ok(None);
        }
        
        let summary = fs::read_to_string(&paths.summary).await?;
        let metadata = fs::metadata(&paths.summary).await?;
        let generated_at = DateTime::from(metadata.created()?);
        
        Ok(Some(JournalSummary {
            cycle_date: *cycle_date,
            summary,
            generated_at,
        }))
    }

    /// Save a journal prompt
    pub async fn save_prompt(&self, prompt: &JournalPrompt) -> Result<(), Box<dyn std::error::Error>> {
        self.ensure_date_directory(&prompt.cycle_date).await?;
        let paths = self.get_file_paths(&prompt.cycle_date);
        
        let prompt_path = match prompt.prompt_number {
            1 => paths.prompt1,
            2 => paths.prompt2,
            3 => paths.prompt3,
            n if n > 3 => {
                // For prompts beyond 3, create additional files in the date directory
                let date_dir = self.base_path.join(prompt.cycle_date.to_string());
                date_dir.join(format!("prompt{}.txt", n))
            },
            _ => return Err("Invalid prompt number".into()),
        };
        
        let mut file = fs::File::create(&prompt_path).await?;
        file.write_all(prompt.prompt.as_bytes()).await?;
        
        Ok(())
    }

    /// Load a journal prompt
    pub async fn load_prompt(&self, cycle_date: &CycleDate, prompt_number: u8) -> Result<Option<JournalPrompt>, Box<dyn std::error::Error>> {
        let paths = self.get_file_paths(cycle_date);
        
        let prompt_path = match prompt_number {
            1 => paths.prompt1,
            2 => paths.prompt2,
            3 => paths.prompt3,
            n if n > 3 => {
                // For prompts beyond 3, check additional files in the date directory
                let date_dir = self.base_path.join(cycle_date.to_string());
                date_dir.join(format!("prompt{}.txt", n))
            },
            _ => return Err("Invalid prompt number".into()),
        };
        
        if !prompt_path.exists() {
            return Ok(None);
        }
        
        let prompt = fs::read_to_string(&prompt_path).await?;
        let metadata = fs::metadata(&prompt_path).await?;
        let generated_at = DateTime::from(metadata.created()?);
        
        // Determine prompt type based on cycle date
        let prompt_type = if cycle_date.is_first_day_of_year() {
            PromptType::YearlyReflection
        } else if cycle_date.is_first_day_of_month() {
            PromptType::MonthlyReflection
        } else if cycle_date.is_first_day_of_week() {
            PromptType::WeeklyReflection
        } else {
            PromptType::Daily
        };
        
        Ok(Some(JournalPrompt {
            cycle_date: *cycle_date,
            prompt,
            prompt_number,
            generated_at,
            prompt_type,
        }))
    }

    /// Save a journal status update
    pub async fn save_status(&self, cycle_date: &CycleDate, status: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.ensure_date_directory(cycle_date).await?;
        let paths = self.get_file_paths(cycle_date);
        
        let mut file = fs::File::create(&paths.status).await?;
        file.write_all(status.as_bytes()).await?;
        
        Ok(())
    }

    /// Load a journal status
    pub async fn load_status(&self, cycle_date: &CycleDate) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let paths = self.get_file_paths(cycle_date);
        
        if !paths.status.exists() {
            return Ok(None);
        }
        
        let status = fs::read_to_string(&paths.status).await?;
        Ok(Some(status))
    }

    /// Find entries that need summaries
    pub async fn find_entries_needing_summaries(&self) -> Result<Vec<CycleDate>, Box<dyn std::error::Error>> {
        let mut entries_needing_summaries = Vec::new();
        
        // Read all date directories in the base directory
        let mut dir_entries = fs::read_dir(&self.base_path).await?;
        
        while let Some(entry) = dir_entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                let dir_name = entry.file_name();
                let dir_name_str = dir_name.to_string_lossy();
                
                // Check if this is a valid date directory (5 characters)
                if dir_name_str.len() == 5 {
                    if let Ok(cycle_date) = CycleDate::from_string(&dir_name_str) {
                        // Check if entry exists and summary doesn't
                        let paths = self.get_file_paths(&cycle_date);
                        if paths.entry.exists() && !paths.summary.exists() {
                            entries_needing_summaries.push(cycle_date);
                        }
                    }
                }
            }
        }
        
        Ok(entries_needing_summaries)
    }

    /// Find entries that need status files
    pub async fn find_entries_needing_status(&self) -> Result<Vec<CycleDate>, Box<dyn std::error::Error>> {
        let mut entries_needing_status = Vec::new();
        
        // Read all date directories in the base directory
        let mut dir_entries = fs::read_dir(&self.base_path).await?;
        
        while let Some(entry) = dir_entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                let dir_name = entry.file_name();
                let dir_name_str = dir_name.to_string_lossy();
                
                // Check if this is a valid date directory (5 characters)
                if dir_name_str.len() == 5 {
                    if let Ok(cycle_date) = CycleDate::from_string(&dir_name_str) {
                        // Check if entry exists and status doesn't
                        let paths = self.get_file_paths(&cycle_date);
                        if paths.entry.exists() && !paths.status.exists() {
                            entries_needing_status.push(cycle_date);
                        }
                    }
                }
            }
        }
        
        Ok(entries_needing_status)
    }

    /// Get past entries for prompt generation based on prompt type
    pub async fn get_context_for_prompt(&self, cycle_date: &CycleDate) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let mut context = Vec::new();
        
        if cycle_date.is_first_day_of_year() {
            // Get monthly reflections from past year
            for month in 0..13 {
                let mut past_date = *cycle_date;
                past_date.year_cycle = if past_date.year_cycle > 0 { past_date.year_cycle - 1 } else { 99 };
                past_date.month = month;
                past_date.week = 0;
                past_date.day = 0;
                
                if let Ok(Some(entry)) = self.load_entry(&past_date).await {
                    context.push(format!("Month {} reflection: {}", month, entry.content));
                }
            }
        } else if cycle_date.is_first_day_of_month() {
            // Get weekly reflections from past month
            for week in 0..4 {
                let mut past_date = *cycle_date;
                if past_date.month > 0 {
                    past_date.month -= 1;
                } else {
                    past_date.month = 12;
                    past_date.year_cycle = if past_date.year_cycle > 0 { past_date.year_cycle - 1 } else { 99 };
                }
                past_date.week = week;
                past_date.day = 0;
                
                if let Ok(Some(entry)) = self.load_entry(&past_date).await {
                    context.push(format!("Week {} reflection: {}", week, entry.content));
                }
            }
        } else if cycle_date.is_first_day_of_week() {
            // Get full entries from past 7 days
            let past_week = cycle_date.previous_week();
            for past_date in past_week {
                if let Ok(Some(entry)) = self.load_entry(&past_date).await {
                    context.push(format!("Day {}: {}", past_date.to_string(), entry.content));
                }
            }
        } else {
            // Get summaries from past 7 days
            let past_week = cycle_date.previous_week();
            for past_date in past_week {
                if let Ok(Some(summary)) = self.load_summary(&past_date).await {
                    context.push(format!("Day {}: {}", past_date.to_string(), summary.summary));
                }
            }
        }
        
        Ok(context)
    }

    /// Save a writing sample
    pub async fn save_writing_sample(&self, sample: &WritingSample) -> Result<(), Box<dyn std::error::Error>> {
        self.ensure_date_directory(&sample.cycle_date).await?;
        let paths = self.get_file_paths(&sample.cycle_date);
        let sample_path = paths.sample_path(sample.sample_number);
        
        let mut file = fs::File::create(&sample_path).await?;
        file.write_all(sample.content.as_bytes()).await?;
        
        Ok(())
    }

    /// Load a writing sample
    pub async fn load_writing_sample(&self, cycle_date: &CycleDate, sample_number: u8) -> Result<Option<WritingSample>, Box<dyn std::error::Error>> {
        let paths = self.get_file_paths(cycle_date);
        let sample_path = paths.sample_path(sample_number);
        
        if !sample_path.exists() {
            return Ok(None);
        }
        
        let content = fs::read_to_string(&sample_path).await?;
        let metadata = fs::metadata(&sample_path).await?;
        let uploaded_at = DateTime::from(metadata.created()?);
        
        Ok(Some(WritingSample {
            cycle_date: *cycle_date,
            sample_number,
            content,
            uploaded_at,
        }))
    }

    /// List all writing sample numbers for a given date
    pub async fn list_writing_samples(&self, cycle_date: &CycleDate) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let date_dir = self.base_path.join(cycle_date.to_string());
        
        if !date_dir.exists() {
            return Ok(Vec::new());
        }
        
        let mut sample_numbers = Vec::new();
        let mut dir_entries = fs::read_dir(&date_dir).await?;
        
        while let Some(entry) = dir_entries.next_entry().await? {
            if let Some(filename) = entry.file_name().to_str() {
                // Match files like "sample1.txt", "sample2.txt", etc.
                if filename.starts_with("sample") && filename.ends_with(".txt") && !filename.contains("_summary") {
                    if let Some(num_str) = filename.strip_prefix("sample").and_then(|s| s.strip_suffix(".txt")) {
                        if let Ok(num) = num_str.parse::<u8>() {
                            sample_numbers.push(num);
                        }
                    }
                }
            }
        }
        
        sample_numbers.sort_unstable();
        Ok(sample_numbers)
    }

    /// Delete a writing sample and its summary
    pub async fn delete_writing_sample(&self, cycle_date: &CycleDate, sample_number: u8) -> Result<(), Box<dyn std::error::Error>> {
        let paths = self.get_file_paths(cycle_date);
        let sample_path = paths.sample_path(sample_number);
        let summary_path = paths.sample_summary_path(sample_number);
        
        if sample_path.exists() {
            fs::remove_file(sample_path).await?;
        }
        
        if summary_path.exists() {
            fs::remove_file(summary_path).await?;
        }
        
        Ok(())
    }

    /// Save a writing sample summary
    pub async fn save_sample_summary(&self, summary: &WritingSampleSummary) -> Result<(), Box<dyn std::error::Error>> {
        self.ensure_date_directory(&summary.cycle_date).await?;
        let paths = self.get_file_paths(&summary.cycle_date);
        let summary_path = paths.sample_summary_path(summary.sample_number);
        
        let mut file = fs::File::create(&summary_path).await?;
        file.write_all(summary.summary.as_bytes()).await?;
        
        Ok(())
    }

    /// Load a writing sample summary
    pub async fn load_sample_summary(&self, cycle_date: &CycleDate, sample_number: u8) -> Result<Option<WritingSampleSummary>, Box<dyn std::error::Error>> {
        let paths = self.get_file_paths(cycle_date);
        let summary_path = paths.sample_summary_path(sample_number);
        
        if !summary_path.exists() {
            return Ok(None);
        }
        
        let summary = fs::read_to_string(&summary_path).await?;
        let metadata = fs::metadata(&summary_path).await?;
        let generated_at = DateTime::from(metadata.created()?);
        
        Ok(Some(WritingSampleSummary {
            cycle_date: *cycle_date,
            sample_number,
            summary,
            generated_at,
        }))
    }

    /// Get all writing sample summaries across all dates (for context building)
    pub async fn get_all_sample_summaries(&self) -> Result<Vec<(CycleDate, u8, String)>, Box<dyn std::error::Error>> {
        let mut all_summaries = Vec::new();
        
        // Read all date directories
        let mut dir_entries = fs::read_dir(&self.base_path).await?;
        
        while let Some(entry) = dir_entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                let dir_name = entry.file_name();
                let dir_name_str = dir_name.to_string_lossy();
                
                // Check if this is a valid date directory (5 characters)
                if dir_name_str.len() == 5 {
                    if let Ok(cycle_date) = CycleDate::from_string(&dir_name_str) {
                        // Find all sample summaries in this directory
                        let date_dir = entry.path();
                        let mut date_dir_entries = fs::read_dir(&date_dir).await?;
                        
                        while let Some(file_entry) = date_dir_entries.next_entry().await? {
                            if let Some(filename) = file_entry.file_name().to_str() {
                                // Match files like "sample1_summary.txt"
                                if filename.starts_with("sample") && filename.ends_with("_summary.txt") {
                                    if let Some(num_str) = filename.strip_prefix("sample")
                                        .and_then(|s| s.strip_suffix("_summary.txt")) {
                                        if let Ok(num) = num_str.parse::<u8>() {
                                            if let Ok(summary) = fs::read_to_string(file_entry.path()).await {
                                                all_summaries.push((cycle_date, num, summary));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        Ok(all_summaries)
    }

    /// Find writing samples that need summaries
    pub async fn find_samples_needing_summaries(&self) -> Result<Vec<(CycleDate, u8)>, Box<dyn std::error::Error>> {
        let mut samples_needing_summaries = Vec::new();
        
        // Read all date directories
        let mut dir_entries = fs::read_dir(&self.base_path).await?;
        
        while let Some(entry) = dir_entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                let dir_name = entry.file_name();
                let dir_name_str = dir_name.to_string_lossy();
                
                // Check if this is a valid date directory (5 characters)
                if dir_name_str.len() == 5 {
                    if let Ok(cycle_date) = CycleDate::from_string(&dir_name_str) {
                        let paths = self.get_file_paths(&cycle_date);
                        
                        // Find all samples in this directory
                        let sample_numbers = self.list_writing_samples(&cycle_date).await?;
                        for sample_number in sample_numbers {
                            let summary_path = paths.sample_summary_path(sample_number);
                            if !summary_path.exists() {
                                samples_needing_summaries.push((cycle_date, sample_number));
                            }
                        }
                    }
                }
            }
        }
        
        Ok(samples_needing_summaries)
    }
}

/// File paths for a journal day
pub struct JournalFilePaths {
    pub entry: PathBuf,
    pub summary: PathBuf,
    pub status: PathBuf,
    pub prompt1: PathBuf,
    pub prompt2: PathBuf,
    pub prompt3: PathBuf,
    date_dir: PathBuf,  // Store for helper methods
}

impl JournalFilePaths {
    /// Get the path for a writing sample
    pub fn sample_path(&self, sample_number: u8) -> PathBuf {
        self.date_dir.join(format!("sample{}.txt", sample_number))
    }

    /// Get the path for a writing sample summary
    pub fn sample_summary_path(&self, sample_number: u8) -> PathBuf {
        self.date_dir.join(format!("sample{}_summary.txt", sample_number))
    }
}
