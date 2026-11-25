//! Serializable capability overrides for custom Responses-compatible providers.
//!
//! Unspecified capabilities inherit the provider's existing defaults. Model and
//! turn settings may further restrict the capabilities declared here.

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

/// Overrides for the optional API features supported by a custom provider.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ModelProviderCapabilities {
    /// Whether the provider accepts explicit `access_programs.cyber` selections.
    /// Defaults to enabled for the built-in OpenAI provider and disabled otherwise.
    /// This does not grant program access or change authentication.
    pub cyber_access_programs: Option<bool>,
    /// Whether hosted web search may access the live web.
    pub external_web_access: Option<bool>,
    /// Remote context-compaction protocol; omission preserves provider defaults.
    pub remote_compaction: Option<RemoteCompactionSupport>,
    /// WitLex (D215): the provider answers a rate limit with HTTP 429 and the error code
    /// `insufficient_quota` (Qwen's `Throttling.AllocationQuota`, a per-minute token limit), which
    /// otherwise reads as OpenAI's exhausted account quota. When true, that 429 is a retryable rate
    /// limit that keeps the provider's message and retry-after. Defaults to false.
    pub insufficient_quota_is_rate_limit: Option<bool>,
    /// WitLex (D217): the provider never encrypts tool arguments, so a function call that arrives
    /// without `encrypted_function_args` carries its arguments in plain text. When true, such a call
    /// is marked as OpenAI's server marks plain-text arguments (an empty list); Multi-agent V2's
    /// spawn_agent, send_message and followup_task then deliver their `message` as plain text
    /// instead of an `encrypted_content` part the provider rejects. Defaults to false.
    pub plaintext_function_arguments: Option<bool>,
}

/// Remote context-compaction protocols supported by a model provider.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RemoteCompactionSupport {
    /// Use local compaction instead of a provider compaction protocol.
    Unsupported,
    /// Send `compaction_trigger` items over the Responses endpoint.
    V2,
}
