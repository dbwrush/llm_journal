use llm_journal::personalization::PersonalizationConfig;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load personalization config
    let config = PersonalizationConfig::load("journal")?;
    
    println!("=== TEMPORAL AWARENESS TEST ===\n");
    
    // Test temporal context
    let temporal_context = config.get_temporal_context();
    println!("Current temporal context:");
    println!("{}", temporal_context);
    
    // Test upcoming holidays
    let upcoming = config.get_upcoming_holidays();
    println!("Upcoming holidays ({} found):", upcoming.len());
    for holiday in upcoming.iter().take(5) {
        println!("- {} ({}): {}", holiday.name, holiday.date, holiday.category);
        if let Some(desc) = &holiday.description {
            println!("  Description: {}", desc);
        }
    }
    
    // Test enriched context
    println!("\n=== ENRICHED CONTEXT SAMPLE ===");
    let base_context = "Recent journal entries show excitement about upcoming projects";
    let enriched = config.enrich_context(base_context);
    println!("{}", enriched);
    
    Ok(())
}
