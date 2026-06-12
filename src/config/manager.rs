use std::sync::Arc;

use tokio::{runtime::Runtime, sync::RwLock};

use crate::config::model::RuntimeConfig;

#[derive(Debug, Clone)]
pub struct ConfigManager {
    current: Arc<RwLock<RuntimeConfig>>,
}

impl ConfigManager {
    pub fn new(initial: RuntimeConfig) -> Self {
        Self {
            current: Arc::new(RwLock::new(initial)),
        }
    }

    pub async fn get_config(&self) -> RuntimeConfig {
        self.current.read().await.clone()
    }

    pub async fn replace_config(&self, next: RuntimeConfig) {
        *self.current.write().await = next;
    }
}
