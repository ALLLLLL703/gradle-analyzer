use std::sync::{Arc, OnceLock};

use tokio::sync::RwLock;

use crate::config::{default::default_runtime_config, model::RuntimeConfig};

static GLOBAL_CONFIG_MANGER: OnceLock<Arc<ConfigManager>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct ConfigManager {
    current: Arc<RwLock<RuntimeConfig>>,
}

impl ConfigManager {
    pub fn global() -> Arc<Self> {
        GLOBAL_CONFIG_MANGER
            .get_or_init(|| Arc::new(Self::new(default_runtime_config())))
            .clone()
    }
    pub fn new(initial: RuntimeConfig) -> Self {
        Self {
            current: Arc::new(RwLock::new(initial)),
        }
    }

    pub async fn get_config(&self) -> RuntimeConfig {
        self.current.read().await.clone()
    }

    pub fn current_config_or_default(&self) -> RuntimeConfig {
        self.current
            .try_read()
            .map(|config| config.clone())
            .unwrap_or_else(|_| default_runtime_config())
    }

    pub async fn replace_config(&self, next: RuntimeConfig) {
        *self.current.write().await = next;
    }
}
