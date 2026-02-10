use std::path::PathBuf;

use crate::cascade::cache::ExactCache;
use crate::error::Result;
use crate::scope::ScopeLevel;
use crate::storage::jsonl::JsonlStorage;
use crate::storage::StorageBackend;

/// Stream decisions in real time.
/// Watches the JSONL rule files for changes and prints new decisions.
pub async fn run_monitor() -> Result<()> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let project_root = cwd.join(".hookwise");
    let rules_dir = project_root.join("rules");

    eprintln!(
        "hookwise: monitoring decisions in {}",
        rules_dir.display()
    );
    eprintln!("Press Ctrl+C to stop.\n");

    // Track file sizes to detect new entries
    let mut last_sizes = std::collections::HashMap::new();
    for file in &["allow.jsonl", "deny.jsonl", "ask.jsonl"] {
        let path = rules_dir.join(file);
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        last_sizes.insert(file.to_string(), size);
    }

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        for file in &["allow.jsonl", "deny.jsonl", "ask.jsonl"] {
            let path = rules_dir.join(file);
            let current_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let last_size = last_sizes.get(*file).copied().unwrap_or(0);

            if current_size > last_size {
                // New content added -- read and display new lines
                if let Ok(contents) = std::fs::read_to_string(&path) {
                    // Find a safe UTF-8 char boundary at or after the byte offset
                    let byte_offset = last_size as usize;
                    let safe_offset = if byte_offset >= contents.len() {
                        contents.len()
                    } else if contents.is_char_boundary(byte_offset) {
                        byte_offset
                    } else {
                        // Scan forward to the next char boundary
                        (byte_offset..contents.len())
                            .find(|&i| contents.is_char_boundary(i))
                            .unwrap_or(contents.len())
                    };
                    let new_content = &contents[safe_offset..];
                    for line in new_content.lines() {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        if let Ok(record) =
                            serde_json::from_str::<crate::decision::DecisionRecord>(trimmed)
                        {
                            println!(
                                "[{}] {} {} {} (tier: {:?}, confidence: {:.2}) -- {}",
                                record.timestamp.format("%H:%M:%S"),
                                record.decision,
                                record.key.tool,
                                record.key.role,
                                record.metadata.tier,
                                record.metadata.confidence,
                                record.metadata.reason,
                            );
                        }
                    }
                }

                last_sizes.insert(file.to_string(), current_size);
            }
        }
    }
}

/// Show cache hit rates and decision distribution.
pub async fn run_stats() -> Result<()> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let project_root = cwd.join(".hookwise");
    let global_root = dirs_global();

    let storage = JsonlStorage::new(project_root, global_root, None);

    let decisions = storage.load_decisions(ScopeLevel::Project)?;

    // Build an ExactCache to get stats
    let cache = ExactCache::new();
    cache.load_from(decisions.clone());
    let stats = cache.stats();

    println!("hookwise statistics");
    println!("=======================");
    println!("Total cached decisions: {}", stats.total_entries);
    println!("  Allow: {}", stats.allow_entries);
    println!("  Deny:  {}", stats.deny_entries);
    println!("  Ask:   {}", stats.ask_entries);
    println!();

    // Count by tier
    let mut tier_counts = std::collections::HashMap::new();
    let mut role_counts = std::collections::HashMap::new();
    let mut tool_counts = std::collections::HashMap::new();

    for record in &decisions {
        *tier_counts
            .entry(format!("{:?}", record.metadata.tier))
            .or_insert(0) += 1;
        *role_counts.entry(record.key.role.clone()).or_insert(0) += 1;
        *tool_counts.entry(record.key.tool.clone()).or_insert(0) += 1;
    }

    println!("By tier:");
    for (tier, count) in &tier_counts {
        println!("  {}: {}", tier, count);
    }

    println!("\nBy role:");
    for (role, count) in &role_counts {
        println!("  {}: {}", role, count);
    }

    println!("\nBy tool:");
    for (tool, count) in &tool_counts {
        println!("  {}: {}", tool, count);
    }

    Ok(())
}

fn dirs_global() -> PathBuf {
    crate::config::dirs_global()
}
