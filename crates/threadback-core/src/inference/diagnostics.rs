use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::StructuredOutput;

/// What is recorded about one request to the model. Every field is a number,
/// a model name, a host, or a fixed label: there is no field that could hold
/// the evidence that was sent, the reply, or the authorization header, so none
/// of them can end up in a log by accident.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelCallRecord {
    pub model_id: String,
    /// The host only, never the full URL.
    pub endpoint_host: String,
    /// `ok`, or the kind of failure: `authentication`, `rate_limited`,
    /// `network`, `timeout`, `server`, `rejected`, `cancelled`, `unreadable`.
    pub status: String,
    pub http_status: Option<u16>,
    #[ts(type = "number")]
    pub latency_ms: u64,
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
    pub finish_reason: Option<String>,
    /// The response format this request actually used.
    pub structured_output: StructuredOutput,
    /// 1 for the first request of a generation, 2 for the correction.
    pub attempt: u8,
}
