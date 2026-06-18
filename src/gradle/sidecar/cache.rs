//! A pure freshness gate for imported [`SidecarModel`]s, keyed by Gradle version + the
//! resolved classpath fingerprint.
//!
//! A full Gradle sync is slow, so an imported model is cached and reused while its inputs
//! are unchanged. The cache key is intentionally coarse — the Gradle version plus an
//! order-insensitive fingerprint of the resolved classpath jars — which is enough to detect
//! the common "dependencies changed" invalidation. Real file-mtime / wrapper-property
//! freshness gating lands in Task 16; this module proves the staleness *contract*: a lookup
//! under a key that differs from the cached one is a miss, which the service turns into
//! [`crate::gradle::sidecar::SidecarFailure::StaleCache`] when a fresh import is unavailable.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::gradle::sidecar::model::SidecarModel;

/// The identity of a cached model: the Gradle version and a classpath fingerprint.
///
/// Two keys compare equal only when both fields match, so any change to the Gradle version
/// or the resolved classpath produces a different key and a cache miss.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::sidecar::cache::ModelCacheKey;
///
/// let a = ModelCacheKey::new("8.10", &["/r/a.jar".into(), "/r/b.jar".into()]);
/// // Order-insensitive: the same jars in any order yield the same key.
/// let b = ModelCacheKey::new("8.10", &["/r/b.jar".into(), "/r/a.jar".into()]);
/// assert_eq!(a, b);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModelCacheKey {
    /// The Gradle version that produced the model.
    gradle_version: String,
    /// A stable, order-insensitive fingerprint of the resolved classpath jars.
    classpath_fingerprint: String,
}

impl ModelCacheKey {
    /// Builds a key from a Gradle version and the resolved classpath jar paths.
    pub fn new(gradle_version: &str, classpath_jars: &[String]) -> Self {
        Self {
            gradle_version: gradle_version.to_string(),
            classpath_fingerprint: fingerprint_classpath(classpath_jars),
        }
    }

    /// Builds the key that *would* identify `model`, for storing it after an import.
    pub fn of_model(model: &SidecarModel) -> Self {
        Self::new(&model.gradle_version, &model.classpath_jars)
    }
}

/// Computes a stable, order-insensitive fingerprint of the classpath jar set.
///
/// The jars are sorted before hashing so reordering the resolved classpath does not change
/// the fingerprint; only adding, removing, or renaming a jar does.
pub fn fingerprint_classpath(classpath_jars: &[String]) -> String {
    let mut sorted: Vec<&String> = classpath_jars.iter().collect();
    sorted.sort();
    let mut hasher = DefaultHasher::new();
    sorted.len().hash(&mut hasher);
    for jar in sorted {
        jar.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

/// A single-entry model cache gated by [`ModelCacheKey`].
///
/// Holds at most one `(key, model)` pair — the last successful import. A
/// [`ModelCache::get`] under a non-matching key returns `None` (a stale/miss), which the
/// service maps to [`crate::gradle::sidecar::SidecarFailure::StaleCache`] when it cannot
/// produce a fresh import.
#[derive(Debug, Clone, Default)]
pub struct ModelCache {
    entry: Option<(ModelCacheKey, SidecarModel)>,
}

impl ModelCache {
    /// Creates an empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the cached model iff `key` matches the stored entry's key.
    ///
    /// A `None` result means either the cache is empty or the key changed (stale): the
    /// caller must re-import or degrade.
    pub fn get(&self, key: &ModelCacheKey) -> Option<&SidecarModel> {
        match &self.entry {
            Some((stored, model)) if stored == key => Some(model),
            _ => None,
        }
    }

    /// Stores `model` under its own derived key, replacing any prior entry.
    pub fn store(&mut self, model: SidecarModel) {
        let key = ModelCacheKey::of_model(&model);
        self.entry = Some((key, model));
    }

    /// Returns `true` when the stored entry (if any) is stale for `key`.
    ///
    /// `true` for both an empty cache and a key mismatch — either way `key` is not served
    /// from cache.
    pub fn is_stale_for(&self, key: &ModelCacheKey) -> bool {
        self.get(key).is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(version: &str, jars: &[&str]) -> SidecarModel {
        SidecarModel {
            gradle_version: version.to_string(),
            classpath_jars: jars.iter().map(|s| s.to_string()).collect(),
            ..SidecarModel::default()
        }
    }

    #[test]
    fn fingerprint_is_order_insensitive_but_content_sensitive() {
        let a = fingerprint_classpath(&["a.jar".into(), "b.jar".into()]);
        let b = fingerprint_classpath(&["b.jar".into(), "a.jar".into()]);
        assert_eq!(a, b, "reordering must not change the fingerprint");

        let c = fingerprint_classpath(&["a.jar".into(), "c.jar".into()]);
        assert_ne!(a, c, "changing a jar must change the fingerprint");
    }

    #[test]
    fn store_then_get_under_matching_key_hits() {
        let mut cache = ModelCache::new();
        cache.store(model("8.10", &["a.jar", "b.jar"]));

        let key = ModelCacheKey::new("8.10", &["b.jar".into(), "a.jar".into()]);
        let hit = cache.get(&key).expect("cache hit under matching key");
        assert_eq!(hit.gradle_version, "8.10");
        assert!(!cache.is_stale_for(&key));
    }

    #[test]
    fn changed_classpath_is_a_stale_miss() {
        let mut cache = ModelCache::new();
        cache.store(model("8.10", &["a.jar"]));

        let changed = ModelCacheKey::new("8.10", &["a.jar".into(), "new.jar".into()]);
        assert!(cache.get(&changed).is_none(), "added jar invalidates the key");
        assert!(cache.is_stale_for(&changed));
    }

    #[test]
    fn changed_gradle_version_is_a_stale_miss() {
        let mut cache = ModelCache::new();
        cache.store(model("8.10", &["a.jar"]));

        let upgraded = ModelCacheKey::new("8.11", &["a.jar".into()]);
        assert!(cache.is_stale_for(&upgraded), "version bump invalidates the key");
    }

    #[test]
    fn empty_cache_is_stale_for_any_key() {
        let cache = ModelCache::new();
        let key = ModelCacheKey::new("8.10", &["a.jar".into()]);
        assert!(cache.is_stale_for(&key));
        assert!(cache.get(&key).is_none());
    }
}
