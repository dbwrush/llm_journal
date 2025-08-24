use std::path::Path;

fn main() {
    let config = llm_journal::personalization::PersonalizationConfig::load(Path::new("./journal")).unwrap();
    
    let base_context = "Recent journal entries show feeling stressed about work-life balance and excited about new hiking trail discoveries.";
    let enriched = config.enrich_context(base_context);
    
    println!("=== ENRICHED CONTEXT ===");
    println!("{}", enriched);
    println!("=== END ===");
}
