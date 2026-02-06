//! OpenTelemetry metrics export module.
//!
//! This module provides OpenTelemetry export capability for git-ai metrics,
//! enabling visualization in Grafana dashboards via OTLP protocol.
//!
//! The module is conditionally compiled only when the `otel` feature is enabled.

#[cfg(feature = "otel")]
use opentelemetry::metrics::{Counter, Histogram, Meter, MeterProvider};
#[cfg(feature = "otel")]
use opentelemetry::KeyValue;
#[cfg(feature = "otel")]
use opentelemetry_otlp::WithExportConfig;
#[cfg(feature = "otel")]
use std::collections::HashMap;
#[cfg(feature = "otel")]
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
#[cfg(feature = "otel")]
use opentelemetry_sdk::Resource;
#[cfg(feature = "otel")]
use std::sync::OnceLock;
#[cfg(feature = "otel")]
use std::time::Duration;

#[cfg(feature = "otel")]
use crate::metrics::events::{checkpoint_pos, committed_pos};
#[cfg(feature = "otel")]
use crate::metrics::types::{MetricEvent, MetricEventId};

/// Default OTLP endpoint for gRPC
pub const DEFAULT_OTEL_ENDPOINT: &str = "http://localhost:4317";

/// Default export interval in seconds
pub const DEFAULT_EXPORT_INTERVAL_SECS: u64 = 60;

/// Service name for OpenTelemetry resource
pub const SERVICE_NAME: &str = "git-ai";

/// OTLP transport protocol
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OtelProtocol {
    Grpc,
    Http,
}

impl Default for OtelProtocol {
    fn default() -> Self {
        Self::Grpc
    }
}

/// OpenTelemetry configuration
#[derive(Debug, Clone)]
pub struct OtelConfig {
    /// OTLP endpoint URL (e.g., "http://localhost:4317" for gRPC,
    /// "https://otlp-gateway-prod-us-east-0.grafana.net/otlp" for Grafana Cloud)
    pub endpoint: String,
    /// Whether OTel export is enabled
    pub enabled: bool,
    /// Export interval in seconds
    pub export_interval_secs: u64,
    /// Authorization header value (e.g., "Basic <base64>" for Grafana Cloud)
    pub auth_header: Option<String>,
    /// OTLP transport protocol (gRPC or HTTP/protobuf)
    pub protocol: OtelProtocol,
}

impl Default for OtelConfig {
    fn default() -> Self {
        Self {
            endpoint: DEFAULT_OTEL_ENDPOINT.to_string(),
            enabled: false,
            export_interval_secs: DEFAULT_EXPORT_INTERVAL_SECS,
            auth_header: None,
            protocol: OtelProtocol::default(),
        }
    }
}

impl OtelConfig {
    /// Create OtelConfig from environment variables
    pub fn from_env() -> Self {
        let enabled = std::env::var("GIT_AI_OTEL_ENABLED")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        let endpoint = std::env::var("GIT_AI_OTEL_ENDPOINT")
            .unwrap_or_else(|_| DEFAULT_OTEL_ENDPOINT.to_string());

        let export_interval_secs = std::env::var("GIT_AI_OTEL_EXPORT_INTERVAL")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_EXPORT_INTERVAL_SECS);

        let auth_header = std::env::var("GIT_AI_OTEL_AUTH_HEADER")
            .ok()
            .filter(|s| !s.is_empty());

        let protocol = std::env::var("GIT_AI_OTEL_PROTOCOL")
            .ok()
            .map(|v| match v.to_lowercase().as_str() {
                "http" => OtelProtocol::Http,
                _ => OtelProtocol::Grpc,
            })
            .unwrap_or_default();

        Self {
            endpoint,
            enabled,
            export_interval_secs,
            auth_header,
            protocol,
        }
    }
}

/// OpenTelemetry metrics instruments for git-ai
#[cfg(feature = "otel")]
pub struct OtelMetrics {
    /// Counter for committed human additions
    pub committed_human_additions: Counter<u64>,
    /// Counter for committed AI additions
    pub committed_ai_additions: Counter<u64>,
    /// Counter for git diff added lines
    pub committed_diff_added: Counter<u64>,
    /// Counter for git diff deleted lines
    pub committed_diff_deleted: Counter<u64>,
    /// Counter for AI accepted lines
    pub committed_ai_accepted: Counter<u64>,
    /// Counter for agent usage events
    pub agent_usage_count: Counter<u64>,
    /// Counter for checkpoint events
    pub checkpoint_count: Counter<u64>,
    /// Histogram for checkpoint lines added
    pub checkpoint_lines_added: Histogram<u64>,
    /// Histogram for checkpoint lines deleted
    pub checkpoint_lines_deleted: Histogram<u64>,
}

#[cfg(feature = "otel")]
impl OtelMetrics {
    /// Create new OtelMetrics from a meter
    fn new(meter: &Meter) -> Self {
        Self {
            committed_human_additions: meter
                .u64_counter("git_ai.committed.human_additions")
                .with_description("Number of human-written lines committed")
                .build(),
            committed_ai_additions: meter
                .u64_counter("git_ai.committed.ai_additions")
                .with_description("Number of AI-generated lines committed")
                .build(),
            committed_diff_added: meter
                .u64_counter("git_ai.committed.diff_added")
                .with_description("Total lines added in git diff")
                .build(),
            committed_diff_deleted: meter
                .u64_counter("git_ai.committed.diff_deleted")
                .with_description("Total lines deleted in git diff")
                .build(),
            committed_ai_accepted: meter
                .u64_counter("git_ai.committed.ai_accepted")
                .with_description("Number of AI-generated lines accepted into commit")
                .build(),
            agent_usage_count: meter
                .u64_counter("git_ai.agent_usage.count")
                .with_description("Number of AI agent usage events")
                .build(),
            checkpoint_count: meter
                .u64_counter("git_ai.checkpoint.count")
                .with_description("Number of checkpoint events")
                .build(),
            checkpoint_lines_added: meter
                .u64_histogram("git_ai.checkpoint.lines_added")
                .with_description("Lines added per checkpoint")
                .build(),
            checkpoint_lines_deleted: meter
                .u64_histogram("git_ai.checkpoint.lines_deleted")
                .with_description("Lines deleted per checkpoint")
                .build(),
        }
    }
}

/// Global OTel state
#[cfg(feature = "otel")]
struct OtelState {
    metrics: OtelMetrics,
    _provider: SdkMeterProvider,
}

#[cfg(feature = "otel")]
static OTEL_STATE: OnceLock<Option<OtelState>> = OnceLock::new();

/// Initialize OpenTelemetry with the given configuration.
/// This should be called once at application startup.
/// Returns true if initialization was successful.
#[cfg(feature = "otel")]
pub fn init_otel(config: &OtelConfig) -> bool {
    if !config.enabled {
        let _ = OTEL_STATE.set(None);
        return false;
    }

    let result = OTEL_STATE.get_or_init(|| {
        match init_otel_internal(config) {
            Ok(state) => Some(state),
            Err(e) => {
                eprintln!("[OTel] Failed to initialize OpenTelemetry: {}", e);
                None
            }
        }
    });

    result.is_some()
}

#[cfg(feature = "otel")]
fn init_otel_internal(config: &OtelConfig) -> Result<OtelState, Box<dyn std::error::Error>> {
    use opentelemetry_otlp::MetricExporter;

    let exporter = match config.protocol {
        OtelProtocol::Http => {
            let mut builder = MetricExporter::builder()
                .with_http()
                .with_endpoint(&config.endpoint)
                .with_timeout(Duration::from_secs(10));
            if let Some(auth) = &config.auth_header {
                let mut headers = HashMap::new();
                headers.insert("Authorization".to_string(), auth.clone());
                builder = builder.with_headers(headers);
            }
            builder.build()?
        }
        OtelProtocol::Grpc => {
            let mut builder = MetricExporter::builder()
                .with_tonic()
                .with_endpoint(&config.endpoint)
                .with_timeout(Duration::from_secs(10));
            if let Some(auth) = &config.auth_header {
                let metadata = {
                    let mut map = tonic::metadata::MetadataMap::new();
                    if let Ok(val) = auth.parse() {
                        map.insert("authorization", val);
                    }
                    map
                };
                builder = builder.with_metadata(metadata);
            }
            builder.build()?
        }
    };

    // Create periodic reader
    let reader = PeriodicReader::builder(exporter)
        .with_interval(Duration::from_secs(config.export_interval_secs))
        .build();

    // Create resource with service info
    let resource = Resource::builder()
        .with_attributes(vec![
            KeyValue::new("service.name", SERVICE_NAME),
            KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
        ])
        .build();

    // Create meter provider
    let provider = SdkMeterProvider::builder()
        .with_reader(reader)
        .with_resource(resource)
        .build();

    // Create meter and metrics
    let meter = provider.meter(SERVICE_NAME);
    let metrics = OtelMetrics::new(&meter);

    Ok(OtelState {
        metrics,
        _provider: provider,
    })
}

/// Initialize OTel if not already initialized (lazy initialization)
#[cfg(feature = "otel")]
fn ensure_otel_initialized() -> bool {
    if OTEL_STATE.get().is_some() {
        return OTEL_STATE.get().unwrap().is_some();
    }

    let config = OtelConfig::from_env();
    init_otel(&config)
}

/// Export a metric event to OpenTelemetry.
/// This is a non-blocking operation that won't impact the existing metrics pipeline.
#[cfg(feature = "otel")]
pub fn export_metric_event(event: &MetricEvent) {
    if !ensure_otel_initialized() {
        return;
    }

    let state = match OTEL_STATE.get() {
        Some(Some(state)) => state,
        _ => return,
    };

    // Extract common attributes from the event
    let attrs = extract_attributes(&event.attrs);

    // Route to appropriate handler based on event type
    match MetricEventId::try_from(event.event_id) {
        Ok(MetricEventId::Committed) => {
            export_committed_event(&state.metrics, &event.values, &attrs);
        }
        Ok(MetricEventId::AgentUsage) => {
            export_agent_usage_event(&state.metrics, &attrs);
        }
        Ok(MetricEventId::Checkpoint) => {
            export_checkpoint_event(&state.metrics, &event.values, &attrs);
        }
        Ok(MetricEventId::InstallHooks) => {
            // InstallHooks events are not exported to OTel
        }
        Err(_) => {
            // Unknown event type, skip
        }
    }
}

/// Extract common attributes from event attributes sparse array
#[cfg(feature = "otel")]
fn extract_attributes(attrs: &crate::metrics::types::SparseArray) -> Vec<KeyValue> {
    use crate::metrics::attrs::attr_pos;

    let mut result = Vec::new();

    // Extract string attributes
    let string_attrs = [
        (attr_pos::REPO_URL, "repo_url"),
        (attr_pos::AUTHOR, "author"),
        (attr_pos::COMMIT_SHA, "commit_sha"),
        (attr_pos::BASE_COMMIT_SHA, "base_commit_sha"),
        (attr_pos::BRANCH, "branch"),
        (attr_pos::TOOL, "tool"),
        (attr_pos::MODEL, "model"),
        (attr_pos::PROMPT_ID, "prompt_id"),
    ];

    for (pos, name) in string_attrs {
        if let Some(value) = attrs.get(&pos.to_string()) {
            if let Some(s) = value.as_str() {
                result.push(KeyValue::new(name, s.to_string()));
            }
        }
    }

    result
}

/// Export committed event metrics
#[cfg(feature = "otel")]
fn export_committed_event(
    metrics: &OtelMetrics,
    values: &crate::metrics::types::SparseArray,
    attrs: &[KeyValue],
) {
    // Human additions
    if let Some(value) = values.get(&committed_pos::HUMAN_ADDITIONS.to_string()) {
        if let Some(n) = value.as_u64() {
            metrics.committed_human_additions.add(n, attrs);
        }
    }

    // Git diff added lines
    if let Some(value) = values.get(&committed_pos::GIT_DIFF_ADDED_LINES.to_string()) {
        if let Some(n) = value.as_u64() {
            metrics.committed_diff_added.add(n, attrs);
        }
    }

    // Git diff deleted lines
    if let Some(value) = values.get(&committed_pos::GIT_DIFF_DELETED_LINES.to_string()) {
        if let Some(n) = value.as_u64() {
            metrics.committed_diff_deleted.add(n, attrs);
        }
    }

    // AI additions (array - sum all values for aggregate)
    if let Some(value) = values.get(&committed_pos::AI_ADDITIONS.to_string()) {
        if let Some(arr) = value.as_array() {
            let total: u64 = arr
                .iter()
                .filter_map(|v| v.as_u64())
                .sum();
            if total > 0 {
                metrics.committed_ai_additions.add(total, attrs);
            }
        }
    }

    // AI accepted (array - sum all values for aggregate)
    if let Some(value) = values.get(&committed_pos::AI_ACCEPTED.to_string()) {
        if let Some(arr) = value.as_array() {
            let total: u64 = arr
                .iter()
                .filter_map(|v| v.as_u64())
                .sum();
            if total > 0 {
                metrics.committed_ai_accepted.add(total, attrs);
            }
        }
    }
}

/// Export agent usage event metrics
#[cfg(feature = "otel")]
fn export_agent_usage_event(metrics: &OtelMetrics, attrs: &[KeyValue]) {
    metrics.agent_usage_count.add(1, attrs);
}

/// Export checkpoint event metrics
#[cfg(feature = "otel")]
fn export_checkpoint_event(
    metrics: &OtelMetrics,
    values: &crate::metrics::types::SparseArray,
    attrs: &[KeyValue],
) {
    metrics.checkpoint_count.add(1, attrs);

    // Lines added
    if let Some(value) = values.get(&checkpoint_pos::LINES_ADDED.to_string()) {
        if let Some(n) = value.as_u64() {
            metrics.checkpoint_lines_added.record(n, attrs);
        }
    }

    // Lines deleted
    if let Some(value) = values.get(&checkpoint_pos::LINES_DELETED.to_string()) {
        if let Some(n) = value.as_u64() {
            metrics.checkpoint_lines_deleted.record(n, attrs);
        }
    }
}

/// Shutdown OpenTelemetry gracefully
#[cfg(feature = "otel")]
pub fn shutdown_otel() {
    if let Some(Some(state)) = OTEL_STATE.get() {
        if let Err(e) = state._provider.shutdown() {
            eprintln!("[OTel] Error during shutdown: {:?}", e);
        }
    }
}

// Non-otel feature stubs - these are no-ops when otel feature is disabled

/// Export a metric event to OpenTelemetry (no-op when otel feature is disabled)
#[cfg(not(feature = "otel"))]
pub fn export_metric_event(_event: &crate::metrics::types::MetricEvent) {
    // No-op when otel feature is disabled
}

/// Initialize OpenTelemetry (no-op when otel feature is disabled)
#[cfg(not(feature = "otel"))]
pub fn init_otel(_config: &OtelConfig) -> bool {
    false
}

/// Shutdown OpenTelemetry (no-op when otel feature is disabled)
#[cfg(not(feature = "otel"))]
pub fn shutdown_otel() {
    // No-op when otel feature is disabled
}

#[cfg(all(test, feature = "otel"))]
mod tests {
    use super::*;

    #[test]
    fn test_otel_config_default() {
        let config = OtelConfig::default();
        assert_eq!(config.endpoint, DEFAULT_OTEL_ENDPOINT);
        assert!(!config.enabled);
        assert_eq!(config.export_interval_secs, DEFAULT_EXPORT_INTERVAL_SECS);
    }

    #[test]
    fn test_otel_config_from_env() {
        let config = OtelConfig::from_env();
        assert!(!config.enabled);
        assert_eq!(config.endpoint, DEFAULT_OTEL_ENDPOINT);
        assert!(config.auth_header.is_none());
        assert_eq!(config.protocol, OtelProtocol::Grpc);
    }
}

#[cfg(test)]
mod tests_no_feature {
    use super::*;

    #[test]
    fn test_otel_config_default() {
        let config = OtelConfig::default();
        assert_eq!(config.endpoint, DEFAULT_OTEL_ENDPOINT);
        assert!(!config.enabled);
        assert!(config.auth_header.is_none());
        assert_eq!(config.protocol, OtelProtocol::Grpc);
    }
}
