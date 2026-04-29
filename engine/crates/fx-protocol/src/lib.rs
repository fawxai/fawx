//! Shared protocol definitions for contracts that cross crate or ABI boundaries.
//!
//! WASM skills use these types when a string-only host boundary still needs a
//! typed contract. For example, a network-capable skill can classify a failed
//! request so the host and kernel can stop retrying an impossible route:
//!
//! ```rust
//! use fx_protocol::{FailureClass, StructuredFailure};
//!
//! let failure = StructuredFailure::new(
//!     FailureClass::AuthRequired,
//!     "GitHub rejected the token",
//!     None,
//! );
//! let output_json = serde_json::to_string(&failure).expect("serialize failure");
//! # assert!(output_json.contains("auth_required"));
//! ```
//!
//! Host APIs can also preserve HTTP response shape across the WASM ABI:
//!
//! ```rust
//! use fx_protocol::HttpResponseEnvelope;
//! use std::collections::BTreeMap;
//!
//! let envelope = HttpResponseEnvelope::response(200, BTreeMap::new(), "{}");
//! let output_json = serde_json::to_string(&envelope).expect("serialize response");
//! # assert!(output_json.contains("response"));
//! ```

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Typed classification for a failed tool invocation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    /// The tool requires authentication or a missing credential.
    AuthRequired,
    /// The route could see the resource shape but not the caller's visibility to it.
    VisibilityMismatch,
    /// The requested resource does not exist.
    NotFound,
    /// The tool or route does not support this resource type.
    UnsupportedResource,
    /// The remote service rate limited the request.
    RateLimited,
    /// The request payload or parameters were invalid.
    InvalidRequest,
    /// Transport-level failure such as timeout, DNS, TLS, or connection reset.
    TransientTransport,
    /// The call exceeded a concrete time budget.
    Timeout,
    /// Retrying the same call is not expected to succeed.
    Permanent,
    /// Retrying may succeed without changing the call.
    Transient,
    /// The tool failed, but the tool layer could not classify why.
    Unknown,
    /// The kernel intentionally did not execute the call in this round.
    ///
    /// This is a control-plane disposition, not a tool/runtime failure.
    PolicyDeferred,
}

impl FailureClass {
    pub const ALL: [Self; 12] = [
        Self::AuthRequired,
        Self::VisibilityMismatch,
        Self::NotFound,
        Self::UnsupportedResource,
        Self::RateLimited,
        Self::InvalidRequest,
        Self::TransientTransport,
        Self::Timeout,
        Self::Permanent,
        Self::Transient,
        Self::Unknown,
        Self::PolicyDeferred,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AuthRequired => "auth_required",
            Self::VisibilityMismatch => "visibility_mismatch",
            Self::NotFound => "not_found",
            Self::UnsupportedResource => "unsupported_resource",
            Self::RateLimited => "rate_limited",
            Self::InvalidRequest => "invalid_request",
            Self::TransientTransport => "transient_transport",
            Self::Timeout => "timeout",
            Self::Permanent => "permanent",
            Self::Transient => "transient",
            Self::Unknown => "unknown",
            Self::PolicyDeferred => "policy_deferred",
        }
    }

    /// Returns true when the kernel should stop retrying the same route.
    ///
    /// This is a reroute-classification decision, not a global claim that the
    /// failure can never change in the outside world. For example, `NotFound`
    /// is treated as permanent for same-route retries even though eventually
    /// consistent systems can make a later lookup succeed.
    #[must_use]
    pub const fn is_permanent(self) -> bool {
        matches!(
            self,
            Self::AuthRequired
                | Self::VisibilityMismatch
                | Self::NotFound
                | Self::UnsupportedResource
                | Self::InvalidRequest
                | Self::Permanent
        )
    }

    #[must_use]
    pub const fn is_reroute_relevant(self) -> bool {
        matches!(
            self,
            Self::AuthRequired
                | Self::VisibilityMismatch
                | Self::NotFound
                | Self::UnsupportedResource
                | Self::RateLimited
                | Self::InvalidRequest
                | Self::TransientTransport
                | Self::Timeout
        )
    }

    #[must_use]
    pub const fn prefers_distinct_route(self) -> bool {
        matches!(
            self,
            Self::AuthRequired
                | Self::VisibilityMismatch
                | Self::NotFound
                | Self::UnsupportedResource
                | Self::InvalidRequest
        )
    }
}

/// Diagnostics captured for a structured HTTP failure.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct HttpDiagnostics {
    /// HTTP status code when the host reached the remote application.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
    /// Safe response headers preserved for classification and traces.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    /// Transport-layer failure message when no HTTP response was received.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport_error: Option<String>,
    /// Short response-body snippet for debugging and classification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_snippet: Option<String>,
    /// Whether the body snippet was truncated before serialization.
    #[serde(default)]
    pub body_truncated: bool,
    /// Whether the original body was binary rather than UTF-8 text.
    #[serde(default)]
    pub binary_body: bool,
}

/// Structured failure payload emitted by WASM skills.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StructuredFailure {
    pub class: FailureClass,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<HttpDiagnostics>,
}

impl StructuredFailure {
    #[must_use]
    pub fn new(
        class: FailureClass,
        message: impl Into<String>,
        diagnostics: Option<HttpDiagnostics>,
    ) -> Self {
        Self {
            class,
            message: message.into(),
            diagnostics,
        }
    }
}

/// A side effect outside the local workspace that a skill completed.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ExternalActionKind {
    /// A comment was posted to a GitHub pull request.
    GithubPrComment,
    /// A comment was posted to a GitHub issue.
    GithubIssueComment,
    /// A GitHub pull request review was submitted.
    GithubPrReview,
    /// Changes were pushed to a git remote.
    GitPush,
}

/// Typed evidence that a skill completed an external side effect.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExternalActionEvidence {
    /// The kind of external action that completed.
    pub kind: ExternalActionKind,
    /// A stable URL for the completed action when the provider/tool exposes one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Git remote affected by a push action, when it can be determined.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    /// Git push refspecs/branch targets observed for a push action.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refspecs: Vec<String>,
}

impl ExternalActionEvidence {
    #[must_use]
    pub fn github_pr_comment(url: Option<String>) -> Self {
        Self {
            kind: ExternalActionKind::GithubPrComment,
            url,
            remote: None,
            refspecs: Vec::new(),
        }
    }

    #[must_use]
    pub fn github_issue_comment(url: Option<String>) -> Self {
        Self {
            kind: ExternalActionKind::GithubIssueComment,
            url,
            remote: None,
            refspecs: Vec::new(),
        }
    }

    #[must_use]
    pub fn github_pr_review(url: Option<String>) -> Self {
        Self {
            kind: ExternalActionKind::GithubPrReview,
            url,
            remote: None,
            refspecs: Vec::new(),
        }
    }

    #[must_use]
    pub fn git_push(remote: Option<String>, refspecs: Vec<String>) -> Self {
        Self {
            kind: ExternalActionKind::GitPush,
            url: None,
            remote,
            refspecs,
        }
    }
}

/// Structured success diagnostics emitted by WASM skills.
///
/// Skills include this under a top-level `__fawx_diagnostics` key alongside
/// their normal success output. Hosts consume and strip that key before
/// returning the tool result to the model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct StructuredToolDiagnostics {
    /// Completed external actions observed from this tool call.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub external_actions: Vec<ExternalActionEvidence>,
}

/// Structured HTTP result preserved across the string-only WASM boundary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HttpResponseEnvelope {
    Response(HttpResponseRecord),
    TransportError(HttpTransportError),
}

impl HttpResponseEnvelope {
    #[must_use]
    pub fn response(
        status_code: u16,
        headers: BTreeMap<String, String>,
        body: impl Into<String>,
    ) -> Self {
        Self::Response(HttpResponseRecord {
            status_code,
            headers,
            body: body.into(),
        })
    }

    #[must_use]
    pub fn transport_error(message: impl Into<String>) -> Self {
        Self::TransportError(HttpTransportError {
            message: message.into(),
        })
    }
}

/// Successful HTTP response metadata carried over the WASM ABI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HttpResponseRecord {
    pub status_code: u16,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

/// Transport failure for an HTTP request when no application response was received.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HttpTransportError {
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_class_string_tags_round_trip() {
        let expected_tags = [
            "auth_required",
            "visibility_mismatch",
            "not_found",
            "unsupported_resource",
            "rate_limited",
            "invalid_request",
            "transient_transport",
            "timeout",
            "permanent",
            "transient",
            "unknown",
            "policy_deferred",
        ];

        for (class, expected_tag) in FailureClass::ALL.into_iter().zip(expected_tags) {
            let encoded = serde_json::to_string(&class).expect("serialize");
            assert_eq!(encoded, format!("\"{expected_tag}\""));
            let decoded = serde_json::from_str::<FailureClass>(&encoded).expect("deserialize");
            assert_eq!(decoded, class);
            assert_eq!(class.as_str(), expected_tag);
        }
    }

    #[test]
    fn http_response_envelope_round_trips_with_tagged_wire_shape() {
        let envelope = HttpResponseEnvelope::response(
            429,
            BTreeMap::from([("retry-after".to_string(), "30".to_string())]),
            r#"{"error":"slow down"}"#,
        );

        let encoded = serde_json::to_string(&envelope).expect("serialize");
        assert_eq!(
            encoded,
            r#"{"kind":"response","status_code":429,"headers":{"retry-after":"30"},"body":"{\"error\":\"slow down\"}"}"#
        );

        let decoded = serde_json::from_str::<HttpResponseEnvelope>(&encoded).expect("deserialize");
        assert_eq!(decoded, envelope);
    }

    #[test]
    fn structured_failure_round_trips_with_http_diagnostics() {
        let failure = StructuredFailure::new(
            FailureClass::AuthRequired,
            "Sign in to GitHub.",
            Some(HttpDiagnostics {
                status_code: Some(401),
                headers: BTreeMap::from([(
                    "www-authenticate".to_string(),
                    "Bearer realm=\"GitHub\"".to_string(),
                )]),
                transport_error: None,
                body_snippet: Some("{\"message\":\"Requires authentication\"}".to_string()),
                body_truncated: false,
                binary_body: false,
            }),
        );

        let encoded = serde_json::to_string(&failure).expect("serialize");
        let decoded = serde_json::from_str::<StructuredFailure>(&encoded).expect("deserialize");
        assert_eq!(decoded, failure);
    }
}
