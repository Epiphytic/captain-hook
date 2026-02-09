use std::path::PathBuf;

use chrono::Utc;

use crate::decision::{CacheKey, Decision, DecisionMetadata, DecisionRecord, DecisionTier};
use crate::error::Result;
use crate::scope::ScopeLevel;
use crate::storage::jsonl::JsonlStorage;
use crate::storage::StorageBackend;

/// Set an explicit permission override.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    role: &str,
    command: Option<&str>,
    tool: Option<&str>,
    file: Option<&str>,
    allow: bool,
    deny: bool,
    ask: bool,
    scope: &str,
) -> Result<()> {
    let decision = if allow {
        Decision::Allow
    } else if deny {
        Decision::Deny
    } else if ask {
        Decision::Ask
    } else {
        eprintln!("captain-hook: must specify --allow, --deny, or --ask");
        std::process::exit(1);
    };

    let scope_level = scope
        .parse::<ScopeLevel>()
        .map_err(|e| crate::error::CaptainHookError::InvalidPolicy { reason: e })?;

    // Build the sanitized input from command/tool/file
    let sanitized_input = match (command, tool) {
        (Some(cmd), _) => cmd.to_string(),
        (None, Some(t)) => format!("tool:{}", t),
        (None, None) => {
            eprintln!("captain-hook: must specify --command or --tool");
            std::process::exit(1);
        }
    };

    let tool_name = tool.unwrap_or("*").to_string();

    let record = DecisionRecord {
        key: CacheKey {
            sanitized_input,
            tool: tool_name.clone(),
            role: role.to_string(),
        },
        decision,
        metadata: DecisionMetadata {
            tier: DecisionTier::Override,
            confidence: 1.0,
            reason: format!(
                "explicit override: {} for role={}, tool={}",
                decision, role, tool_name
            ),
            matched_key: None,
            similarity_score: None,
        },
        timestamp: Utc::now(),
        scope: scope_level,
        file_path: file.map(String::from),
        session_id: "override".to_string(),
    };

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let project_root = cwd.join(".captain-hook");
    let global_root = dirs_global();

    let storage = JsonlStorage::new(project_root, global_root, None);
    storage.save_decision(&record)?;

    eprintln!(
        "captain-hook: override set -- {} {} for role '{}' at scope '{}'",
        decision, tool_name, role, scope
    );

    Ok(())
}

fn dirs_global() -> PathBuf {
    crate::config::dirs_global()
}
