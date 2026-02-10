use std::path::PathBuf;

use crate::cascade::embed_sim::EmbeddingSimilarity;
use crate::cascade::token_sim::TokenJaccard;
use crate::config::PolicyConfig;
use crate::error::Result;
use crate::scope::ScopeLevel;
use crate::storage::jsonl::JsonlStorage;
use crate::storage::StorageBackend;

/// Rebuild vector indexes from rules.
pub async fn run_build() -> Result<()> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let project_root = cwd.join(".hookwise");
    let global_root = dirs_global();
    let policy = PolicyConfig::load_project(&cwd)?;

    let storage = JsonlStorage::new(project_root, global_root, None);
    let decisions = storage.load_decisions(ScopeLevel::Project)?;

    eprintln!(
        "hookwise: rebuilding indexes from {} decision(s)...",
        decisions.len()
    );

    // Rebuild token Jaccard index
    let token_jaccard = TokenJaccard::new(
        policy.similarity.jaccard_threshold,
        policy.similarity.jaccard_min_tokens,
    );
    token_jaccard.load_from(&decisions);
    eprintln!("  Token Jaccard: loaded {} entries", decisions.len());

    // Rebuild embedding similarity index
    match EmbeddingSimilarity::new("default", policy.similarity.embedding_threshold) {
        Ok(es) => {
            es.build_index(&decisions)?;
            eprintln!(
                "  Embedding HNSW: built index with {} entries",
                decisions.len()
            );
        }
        Err(e) => {
            eprintln!("  Embedding HNSW: skipped (model not available: {})", e);
        }
    }

    eprintln!("hookwise: index rebuild complete.");
    Ok(())
}

/// Clear cached decisions.
pub async fn run_invalidate(role: Option<&str>, scope: Option<&str>, all: bool) -> Result<()> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let project_root = cwd.join(".hookwise");
    let global_root = dirs_global();

    let storage = JsonlStorage::new(project_root, global_root, None);

    let scope_level = scope
        .map(|s| {
            s.parse::<ScopeLevel>()
                .map_err(|e| crate::error::HookwiseError::InvalidPolicy { reason: e })
        })
        .transpose()?
        .unwrap_or(ScopeLevel::Project);

    if all {
        storage.invalidate_all(scope_level)?;
        eprintln!(
            "hookwise: cleared all decisions at scope '{}'",
            scope_level
        );
    } else if let Some(role) = role {
        storage.invalidate_role(scope_level, role)?;
        eprintln!(
            "hookwise: cleared decisions for role '{}' at scope '{}'",
            role, scope_level
        );
    } else {
        eprintln!("hookwise: specify --role <role> or --all");
        std::process::exit(1);
    }

    Ok(())
}

fn dirs_global() -> PathBuf {
    crate::config::dirs_global()
}
