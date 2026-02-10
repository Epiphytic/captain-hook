use crate::cascade::human::{load_queue_file, DecisionQueue, HumanResponse};
use crate::decision::Decision;
use crate::error::Result;
use crate::scope::ScopeLevel;

use std::sync::Arc;

/// List pending permission decisions.
pub async fn run_queue() -> Result<()> {
    let state = load_queue_file();
    let pending: Vec<_> = state.pending.values().cloned().collect();

    if pending.is_empty() {
        println!("No pending decisions.");
        return Ok(());
    }

    for decision in &pending {
        println!(
            "ID: {}\n  Role: {}\n  Tool: {}\n  Input: {}\n  File: {}\n  Queued: {}\n",
            decision.id,
            decision.role,
            decision.tool_name,
            truncate(&decision.sanitized_input, 80),
            decision.file_path.as_deref().unwrap_or("-"),
            decision.queued_at,
        );
    }

    println!("{} pending decision(s)", pending.len());
    Ok(())
}

/// Approve a pending decision. Writes the response to the file-backed queue
/// so the blocking `check` process can pick it up.
pub async fn run_approve(id: &str, always_ask: bool, add_rule: bool, scope: &str) -> Result<()> {
    let queue = Arc::new(DecisionQueue::new());

    let rule_scope = if add_rule {
        Some(parse_scope(scope)?)
    } else {
        None
    };

    let response = HumanResponse {
        decision: Decision::Allow,
        always_ask,
        add_rule,
        rule_scope,
    };

    queue.respond(id, response)?;
    eprintln!("hookwise: approved {}", id);

    if always_ask {
        eprintln!("  (cached as 'ask' -- will always prompt)");
    }
    if add_rule {
        eprintln!("  (added as persistent rule at scope '{}')", scope);
    }

    Ok(())
}

/// Deny a pending decision. Writes the response to the file-backed queue
/// so the blocking `check` process can pick it up.
pub async fn run_deny(id: &str, always_ask: bool, add_rule: bool, scope: &str) -> Result<()> {
    let queue = Arc::new(DecisionQueue::new());

    let rule_scope = if add_rule {
        Some(parse_scope(scope)?)
    } else {
        None
    };

    let response = HumanResponse {
        decision: Decision::Deny,
        always_ask,
        add_rule,
        rule_scope,
    };

    queue.respond(id, response)?;
    eprintln!("hookwise: denied {}", id);

    if always_ask {
        eprintln!("  (cached as 'ask' -- will always prompt)");
    }
    if add_rule {
        eprintln!("  (added as persistent rule at scope '{}')", scope);
    }

    Ok(())
}

fn parse_scope(scope: &str) -> Result<ScopeLevel> {
    scope
        .parse::<ScopeLevel>()
        .map_err(|e| crate::error::HookwiseError::InvalidPolicy { reason: e })
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max).collect();
        format!("{}...", truncated)
    }
}
