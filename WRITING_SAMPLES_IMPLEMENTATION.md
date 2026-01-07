# Writing Samples Feature - Implementation Document

## Overview
This document describes the implementation of the writing samples feature for the LLM Journal application. This feature allows users to upload writing samples alongside their daily entries, which are then summarized and intelligently incorporated into prompt generation.

## Architecture Design

### Data Flow
```
User Upload → Writing Sample Files → LLM Summarization → Sample Summaries Archive
                                                                    ↓
                                                     [Prompt Generation Context]
                                                                    ↓
                                            Stage 1: LLM reviews summaries + recent entries
                                                    ↓
                                            Selects relevant samples (if any)
                                                    ↓
                                            Stage 2: Full sample texts loaded
                                                    ↓
                                            Final prompts generated with rich context
```

### File Organization
Each date directory (e.g., `journal/10234/`) will contain:
- **Existing files:**
  - `entry.txt` - Daily journal entry
  - `summary.txt` - Entry summary
  - `status.txt` - User status tracking
  - `prompt1.txt`, `prompt2.txt`, `prompt3.txt` - Generated prompts
  
- **New files:**
  - `sample1.txt`, `sample2.txt`, ... - Raw writing samples
  - `sample1_summary.txt`, `sample2_summary.txt`, ... - LLM-generated summaries

### Data Structures

#### 1. WritingSample (journal.rs)
```rust
pub struct WritingSample {
    pub cycle_date: CycleDate,
    pub sample_number: u8,
    pub content: String,
    pub uploaded_at: DateTime<Local>,
}
```

#### 2. WritingSampleSummary (journal.rs)
```rust
pub struct WritingSampleSummary {
    pub cycle_date: CycleDate,
    pub sample_number: u8,
    pub summary: String,
    pub generated_at: DateTime<Local>,
}
```

#### 3. SampleReference (prompt_generator.rs)
```rust
pub struct SampleReference {
    pub cycle_date: CycleDate,
    pub sample_number: u8,
}
```

## Implementation Details

### Phase 1: Core Data Layer (journal.rs)

#### New Methods in JournalFilePaths
Add helper methods for sample file paths:
```rust
pub fn sample_path(&self, sample_number: u8) -> PathBuf
pub fn sample_summary_path(&self, sample_number: u8) -> PathBuf
```

#### New Methods in JournalManager
1. **save_writing_sample**: Save a writing sample to disk
2. **load_writing_sample**: Load a specific writing sample
3. **list_writing_samples**: List all samples for a date (returns Vec<u8> of sample numbers)
4. **delete_writing_sample**: Delete a sample and its summary
5. **save_sample_summary**: Save a writing sample summary
6. **load_sample_summary**: Load a specific sample summary
7. **get_all_sample_summaries**: Get all sample summaries across all dates (for context)
8. **find_samples_needing_summaries**: Find samples that need summarization

### Phase 2: LLM Integration (llm_worker.rs)

#### New Method: generate_writing_sample_summary
```rust
pub async fn generate_writing_sample_summary(
    &self,
    sample_content: &str,
    cycle_date: &CycleDate,
    sample_number: u8,
    personalization_config: &PersonalizationConfig,
) -> Result<WritingSampleSummary, Box<dyn std::error::Error>>
```

**Prompt template:**
```
You are summarizing a writing sample for context purposes. 
Analyze this writing sample and create a concise summary (2-3 sentences) that captures:
1. The main theme or subject matter
2. The writing style and tone
3. Key emotional or narrative elements
4. Any notable creative techniques

Writing sample:
{content}

Summary:
```

#### Enhanced Method: generate_prompt (modified)
Update to accept optional writing sample context:
```rust
pub async fn generate_prompt(
    &self,
    cycle_date: &CycleDate,
    context: &[String],
    writing_sample_context: &[String], // NEW
    prompt_number: u8,
    prompt_type: PromptType,
    personalization_config: &PersonalizationConfig,
) -> Result<JournalPrompt, Box<dyn std::error::Error>>
```

### Phase 3: Two-Stage Prompt Generation (prompt_generator.rs)

#### Stage 1: Context Selection
New method: **select_relevant_samples**
```rust
async fn select_relevant_samples(
    llm_worker: &LlmWorker,
    recent_summaries: &[String],
    all_sample_summaries: &[(CycleDate, u8, String)],
    personalization_config: &PersonalizationConfig,
) -> Result<Vec<SampleReference>, Box<dyn std::error::Error>>
```

**Selection prompt:**
```
You are helping select relevant writing samples to provide context for today's journal prompts.

Recent journal entry summaries:
{summaries}

Available writing samples (by date and number):
{sample_summaries}

Based on the recent entries, which writing samples (if any) would provide useful context?
Consider thematic connections, emotional continuity, or relevant creative work.

Respond with ONLY a JSON array of objects with "date" and "number" fields, or empty array if none are relevant.
Example: [{"date":"10234","number":1},{"date":"10231","number":2}]
Or: []

Response:
```

#### Stage 2: Prompt Generation with Context
Modify **generate_prompts_unified** to:
1. Gather all sample summaries
2. Call select_relevant_samples
3. Load full texts of selected samples
4. Pass both regular context and sample context to generate_prompt

### Phase 4: Web Interface (handlers.rs)

#### New Routes
1. **POST /journal/upload-sample**
   - Accepts multipart form with file
   - Validates file size and type
   - Saves to appropriate date directory
   - Queues summary generation

2. **GET /journal/samples?date={cycle_date}**
   - Returns JSON list of samples for a date
   - Includes metadata (upload time, summary status)

3. **GET /journal/sample?date={cycle_date}&number={num}**
   - Returns sample content (for viewing/downloading)

4. **DELETE /journal/sample?date={cycle_date}&number={num}**
   - Removes sample and summary

#### New Form Structure
```rust
#[derive(Deserialize)]
pub struct WritingSampleForm {
    pub cycle_date: String,
    pub file_content: String, // Base64 or direct text
    pub filename: String,
}
```

#### Template Updates (templates/journal.html)
Add section for writing samples:
- Upload widget with drag-and-drop
- List of existing samples for the current date
- Delete buttons
- Visual indicator when samples were used in prompt generation

### Phase 5: Background Processing

#### Modified: generate_missing_summaries
Extend to handle writing samples:
```rust
async fn generate_missing_summaries(
    journal_manager: &Arc<JournalManager>,
    llm_worker: &Arc<LlmWorker>,
    personalization_config: &Arc<PersonalizationConfig>,
) -> Result<(), String> {
    // Existing: Handle entry summaries
    // ... existing code ...
    
    // NEW: Handle writing sample summaries
    let samples_needing_summaries = journal_manager
        .find_samples_needing_summaries()
        .await
        .map_err(|e| e.to_string())?;
    
    for (cycle_date, sample_number) in samples_needing_summaries {
        // Load sample, generate summary, save
    }
}
```

### Phase 6: Configuration

#### Update config.toml
Add to `[journal]` section:
```toml
# Maximum writing samples per day
max_samples_per_day = 10

# Maximum sample size in KB
max_sample_size_kb = 500

# Enable two-stage context selection
enable_smart_sample_selection = true
```

#### Update Config struct (config.rs)
```rust
pub struct JournalConfig {
    // ... existing fields ...
    pub max_samples_per_day: u8,
    pub max_sample_size_kb: usize,
    pub enable_smart_sample_selection: bool,
}
```

#### Update prompts.json
Add new templates:
```json
{
  "writing_sample_summary": "You are summarizing a writing sample...",
  "sample_selection": "You are helping select relevant writing samples...",
  "daily_with_samples": "Enhanced daily prompt with writing sample context..."
}
```

### Phase 7: Personalization Integration (personalization.rs)

#### Update PersonalizationConfig
Add method to get sample-related prompts:
```rust
impl PersonalizationConfig {
    pub fn get_sample_summary_prompt(&self, content: &str) -> String {
        // Return writing_sample_summary template with content
    }
    
    pub fn get_sample_selection_prompt(
        &self,
        recent_summaries: &str,
        sample_summaries: &str,
    ) -> String {
        // Return sample_selection template
    }
}
```

## Implementation Order

1. ✅ **Create this documentation**
2. 🔨 **Phase 1**: Add data structures and file I/O (journal.rs)
3. 🔨 **Phase 2**: Add LLM methods (llm_worker.rs)
4. 🔨 **Phase 3**: Implement two-stage generation (prompt_generator.rs)
5. 🔨 **Phase 4**: Add web handlers (handlers.rs)
6. 🔨 **Phase 5**: Update background processing
7. 🔨 **Phase 6**: Update configuration
8. 🔨 **Phase 7**: Update personalization/prompts

## Testing Strategy

### Unit Tests
- File I/O for samples and summaries
- Sample number tracking and limits
- Summary generation formatting

### Integration Tests
- Upload → Summarize → Archive workflow
- Two-stage prompt generation with samples
- Sample selection logic

### Manual Testing
1. Upload multiple samples to a date
2. Verify auto-summarization
3. Check prompt generation includes relevant samples
4. Test sample deletion
5. Verify UI displays samples correctly

## Security Considerations

1. **File Size Limits**: Prevent DOS via large uploads
2. **Content Validation**: Sanitize uploaded text
3. **Path Traversal**: Validate date/sample number parameters
4. **Rate Limiting**: Limit upload frequency per session

## Performance Considerations

1. **Lazy Loading**: Only load full sample texts when needed
2. **Summary Caching**: Keep summaries in memory during prompt generation
3. **Async Processing**: Background summarization doesn't block uploads
4. **Batch Operations**: Process multiple samples in startup checks

## Future Enhancements (Out of Scope)

- Rich text/markdown support
- Sample tagging and categorization
- Full-text search across samples
- Sample version control
- Export/import sample collections
- Sample analytics dashboard

## Migration Path

No migration needed - this is a new feature. Existing journals will continue to work without samples.

Users can gradually add samples as desired.

---

**Document Version**: 1.0  
**Date**: December 28, 2025  
**Status**: Ready for Implementation
