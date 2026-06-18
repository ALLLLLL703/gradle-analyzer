//! The on-disk configuration shape and the precedence merge.
//!
//! [`RawConfig`] mirrors the TOML file with every field optional, so a partial file is
//! valid and unspecified keys fall through to the next layer. [`RawConfig::merge`]
//! implements the documented precedence (later layer wins), and
//! [`RawConfig::into_validated`] fills defaults and validates into a
//! [`GradleAnalyzerConfig`].
//!
//! Precedence (lowest to highest): built-in defaults < user-level file < workspace file.

use serde::Deserialize;

use crate::config::error::ConfigError;
use crate::config::model::{
    FeatureToggles, GradleAnalyzerConfig, LatencyConfig, SidecarConfig, TransportConfig,
    WatcherConfig,
};

/// The raw, fully-optional TOML representation of configuration.
///
/// Unknown keys are rejected (`deny_unknown_fields`) so a typo surfaces as a parse
/// error rather than being silently ignored.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawConfig {
    /// Sidecar section.
    #[serde(default)]
    pub sidecar: RawSidecar,
    /// Latency section.
    #[serde(default)]
    pub latency: RawLatency,
    /// Feature toggles section.
    #[serde(default)]
    pub features: RawFeatures,
    /// Watcher section.
    #[serde(default)]
    pub watcher: RawWatcher,
    /// Transport section.
    #[serde(default)]
    pub transport: RawTransport,
}

/// Raw sidecar section.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawSidecar {
    /// See [`SidecarConfig::request_timeout_ms`].
    pub request_timeout_ms: Option<u64>,
    /// See [`SidecarConfig::model_request_deadline_ms`].
    pub model_request_deadline_ms: Option<u64>,
    /// See [`SidecarConfig::max_concurrent`].
    pub max_concurrent: Option<u32>,
}

/// Raw latency section.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawLatency {
    /// See [`LatencyConfig::static_diagnostics_ms`].
    pub static_diagnostics_ms: Option<u64>,
    /// See [`LatencyConfig::completion_ms`].
    pub completion_ms: Option<u64>,
}

/// Raw feature toggles section.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawFeatures {
    /// See [`FeatureToggles::enable_kotlin_dsl`].
    pub enable_kotlin_dsl: Option<bool>,
    /// See [`FeatureToggles::enable_groovy_dsl`].
    pub enable_groovy_dsl: Option<bool>,
    /// See [`FeatureToggles::enable_sidecar`].
    pub enable_sidecar: Option<bool>,
}

/// Raw watcher section.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawWatcher {
    /// See [`WatcherConfig::debounce_ms`].
    pub debounce_ms: Option<u64>,
}

/// Raw transport section.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawTransport {
    /// See [`TransportConfig::max_message_bytes`].
    pub max_message_bytes: Option<usize>,
}

impl RawConfig {
    /// Overlays `higher` onto `self`, with `higher`'s set fields winning.
    ///
    /// Used to apply the user-level file over defaults, then the workspace file over
    /// that, giving workspace > user > default precedence.
    pub fn merge(self, higher: RawConfig) -> RawConfig {
        RawConfig {
            sidecar: RawSidecar {
                request_timeout_ms: higher
                    .sidecar
                    .request_timeout_ms
                    .or(self.sidecar.request_timeout_ms),
                model_request_deadline_ms: higher
                    .sidecar
                    .model_request_deadline_ms
                    .or(self.sidecar.model_request_deadline_ms),
                max_concurrent: higher.sidecar.max_concurrent.or(self.sidecar.max_concurrent),
            },
            latency: RawLatency {
                static_diagnostics_ms: higher
                    .latency
                    .static_diagnostics_ms
                    .or(self.latency.static_diagnostics_ms),
                completion_ms: higher.latency.completion_ms.or(self.latency.completion_ms),
            },
            features: RawFeatures {
                enable_kotlin_dsl: higher
                    .features
                    .enable_kotlin_dsl
                    .or(self.features.enable_kotlin_dsl),
                enable_groovy_dsl: higher
                    .features
                    .enable_groovy_dsl
                    .or(self.features.enable_groovy_dsl),
                enable_sidecar: higher
                    .features
                    .enable_sidecar
                    .or(self.features.enable_sidecar),
            },
            watcher: RawWatcher {
                debounce_ms: higher.watcher.debounce_ms.or(self.watcher.debounce_ms),
            },
            transport: RawTransport {
                max_message_bytes: higher
                    .transport
                    .max_message_bytes
                    .or(self.transport.max_message_bytes),
            },
        }
    }

    /// Fills defaults for any unset field, then validates into a runtime snapshot.
    pub fn into_validated(self) -> Result<GradleAnalyzerConfig, ConfigError> {
        let defaults = GradleAnalyzerConfig::default();
        let merged = GradleAnalyzerConfig {
            sidecar: SidecarConfig {
                request_timeout_ms: self
                    .sidecar
                    .request_timeout_ms
                    .unwrap_or(defaults.sidecar.request_timeout_ms),
                model_request_deadline_ms: self
                    .sidecar
                    .model_request_deadline_ms
                    .unwrap_or(defaults.sidecar.model_request_deadline_ms),
                max_concurrent: self
                    .sidecar
                    .max_concurrent
                    .unwrap_or(defaults.sidecar.max_concurrent),
            },
            latency: LatencyConfig {
                static_diagnostics_ms: self
                    .latency
                    .static_diagnostics_ms
                    .unwrap_or(defaults.latency.static_diagnostics_ms),
                completion_ms: self
                    .latency
                    .completion_ms
                    .unwrap_or(defaults.latency.completion_ms),
            },
            features: FeatureToggles {
                enable_kotlin_dsl: self
                    .features
                    .enable_kotlin_dsl
                    .unwrap_or(defaults.features.enable_kotlin_dsl),
                enable_groovy_dsl: self
                    .features
                    .enable_groovy_dsl
                    .unwrap_or(defaults.features.enable_groovy_dsl),
                enable_sidecar: self
                    .features
                    .enable_sidecar
                    .unwrap_or(defaults.features.enable_sidecar),
            },
            watcher: WatcherConfig {
                debounce_ms: self
                    .watcher
                    .debounce_ms
                    .unwrap_or(defaults.watcher.debounce_ms),
            },
            transport: TransportConfig {
                max_message_bytes: self
                    .transport
                    .max_message_bytes
                    .unwrap_or(defaults.transport.max_message_bytes),
            },
        };
        merged.validate()?;
        Ok(merged)
    }
}
