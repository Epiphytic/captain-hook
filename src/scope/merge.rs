use super::ScopedDecision;
use crate::decision::Decision;

/// Merge decisions from multiple scopes, applying precedence:
/// DENY > ASK > ALLOW > silent
pub fn merge_decisions(decisions: Vec<ScopedDecision>) -> Option<ScopedDecision> {
    if decisions.is_empty() {
        return None;
    }

    let mut best: Option<ScopedDecision> = None;

    for sd in decisions {
        match &best {
            None => best = Some(sd),
            Some(current) => {
                if decision_priority(&sd.decision) > decision_priority(&current.decision) {
                    best = Some(sd);
                }
            }
        }
    }

    best
}

fn decision_priority(d: &Decision) -> u8 {
    match d {
        Decision::Deny => 3,
        Decision::Ask => 2,
        Decision::Allow => 1,
    }
}
