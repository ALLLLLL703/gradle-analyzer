//! The validated runtime configuration snapshot, [`GradleAnalyzerConfig`].
//!
//! This is the typed, defaulted, validated view of configuration that the rest of the
//! crate reads. Every mutable knob a later task needs lives here so nothing downstream
//! hardcodes a threshold, timeout, latency budget, or feature toggle. Defaults are
//! chosen so an absent config file yields a fully valid snapshot.

use crate::config::error::ConfigError;

/// The complete validated configuration for the language server.
///
/// Grouped into focused sub-sections (composition over one flat struct) so each later
/// task extends the section it owns. Construct via [`GradleAnalyzerConfig::default`] or
/// through the loader, which fills defaults and validates.
///
/// # Example
///
/// ```
/// use gradle_analyzer::config::GradleAnalyzerConfig;
///
/// let cfg = GradleAnalyzerConfig::default();
/// assert!(cfg.watcher.debounce_ms > 0);
/// assert!(cfg.validate().is_ok());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GradleAnalyzerConfig {
    /// Sidecar (JVM Tooling-API helper) limits.
    pub sidecar: SidecarConfig,
    /// Latency budgets for static-tier responsiveness.
    pub latency: LatencyConfig,
    /// Feature toggles for optional behavior.
    pub features: FeatureToggles,
    /// File-watcher / hot-reload tuning.
    pub watcher: WatcherConfig,
    /// LSP transport limits.
    pub transport: TransportConfig,
    /// Completion engine tuning.
    pub completion: CompletionConfig,
}

/// Sidecar process limits used by later sidecar tasks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidecarConfig {
    /// Hard timeout for a sidecar request, in milliseconds.
    pub request_timeout_ms: u64,
    /// Deadline within which a model-dependent request returns pending/empty, in ms.
    pub model_request_deadline_ms: u64,
    /// Maximum concurrent sidecar processes per workspace.
    pub max_concurrent: u32,
}

/// Latency budgets that keep static-tier requests responsive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatencyConfig {
    /// Target budget for static diagnostics, in milliseconds.
    pub static_diagnostics_ms: u64,
    /// Target budget for completion responses, in milliseconds.
    pub completion_ms: u64,
}

/// Optional feature switches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureToggles {
    /// Enable the Kotlin DSL frontend.
    pub enable_kotlin_dsl: bool,
    /// Enable the Groovy DSL frontend.
    pub enable_groovy_dsl: bool,
    /// Enable the advanced (sidecar-backed) tier.
    pub enable_sidecar: bool,
    /// Enable safe, reversible local code actions.
    pub enable_code_actions: bool,
    /// Enable static hover from local facts.
    pub enable_hover: bool,
}

/// File-watcher and hot-reload tuning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatcherConfig {
    /// Debounce window for config file changes, in milliseconds.
    pub debounce_ms: u64,
}

/// LSP transport limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportConfig {
    /// Maximum accepted LSP message size, in bytes.
    pub max_message_bytes: usize,
}

/// Completion engine tuning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionConfig {
    /// Maximum number of completion candidates returned per request.
    pub max_candidates: usize,
}

impl Default for GradleAnalyzerConfig {
    fn default() -> Self {
        Self {
            sidecar: SidecarConfig {
                request_timeout_ms: 30_000,
                model_request_deadline_ms: 1_500,
                max_concurrent: 2,
            },
            latency: LatencyConfig {
                static_diagnostics_ms: 200,
                completion_ms: 150,
            },
            features: FeatureToggles {
                enable_kotlin_dsl: true,
                enable_groovy_dsl: true,
                enable_sidecar: true,
                enable_code_actions: true,
                enable_hover: true,
            },
            watcher: WatcherConfig { debounce_ms: 250 },
            transport: TransportConfig {
                max_message_bytes: 16 * 1024 * 1024,
            },
            completion: CompletionConfig { max_candidates: 50 },
        }
    }
}

impl GradleAnalyzerConfig {
    /// Validates cross-field invariants, returning a typed [`ConfigError`] on failure.
    ///
    /// Validation rejects zero-valued timeouts/budgets/limits that would make later
    /// tasks misbehave, so a malformed-but-parseable file still fails loudly (and
    /// localizably) rather than silently producing a broken runtime.
    pub fn validate(&self) -> Result<(), ConfigError> {
        require_positive_u64("sidecar.request_timeout_ms", self.sidecar.request_timeout_ms)?;
        require_positive_u64(
            "sidecar.model_request_deadline_ms",
            self.sidecar.model_request_deadline_ms,
        )?;
        require_positive_u32("sidecar.max_concurrent", self.sidecar.max_concurrent)?;
        require_positive_u64(
            "latency.static_diagnostics_ms",
            self.latency.static_diagnostics_ms,
        )?;
        require_positive_u64("latency.completion_ms", self.latency.completion_ms)?;
        require_positive_u64("watcher.debounce_ms", self.watcher.debounce_ms)?;
        require_positive_usize("transport.max_message_bytes", self.transport.max_message_bytes)?;
        require_positive_usize("completion.max_candidates", self.completion.max_candidates)?;
        Ok(())
    }
}

fn require_positive_u64(field: &str, value: u64) -> Result<(), ConfigError> {
    if value == 0 {
        return Err(zero_error(field));
    }
    Ok(())
}

fn require_positive_u32(field: &str, value: u32) -> Result<(), ConfigError> {
    if value == 0 {
        return Err(zero_error(field));
    }
    Ok(())
}

fn require_positive_usize(field: &str, value: usize) -> Result<(), ConfigError> {
    if value == 0 {
        return Err(zero_error(field));
    }
    Ok(())
}

fn zero_error(field: &str) -> ConfigError {
    ConfigError::Validation {
        field: field.to_string(),
        reason: "value must be greater than zero".to_string(),
    }
}
