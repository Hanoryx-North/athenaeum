//! Shared value types for provider requests.

use serde::{Deserialize, Serialize};

/// An image passed alongside a prompt.
///
/// Kept as base64 rather than bytes because every provider's API takes it
/// that way, and because it has to survive a JSON hop to the UI process.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentImage {
    pub media_type: String,
    pub data_base64: String,
}
