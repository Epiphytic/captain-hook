// Pending queue management for IPC layer.
// The primary queue logic lives in cascade::human::DecisionQueue.
// This module provides IPC-specific queue serialization if needed.

use crate::cascade::human::PendingDecision;
use crate::error::Result;

/// Serialize pending decisions for IPC transport.
pub fn serialize_pending(decisions: &[PendingDecision]) -> Result<String> {
    serde_json::to_string(decisions).map_err(|e| e.into())
}

/// Deserialize pending decisions from IPC transport.
pub fn deserialize_pending(data: &str) -> Result<Vec<PendingDecision>> {
    serde_json::from_str(data).map_err(|e| e.into())
}
