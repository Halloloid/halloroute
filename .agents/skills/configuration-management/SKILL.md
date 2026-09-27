---
name: configuration-management
description: Environment-based configs, provider API key management, feature flags, runtime configuration updates for RustAgent
used_by: RustAgent Gateway
---

# Configuration Management

Flexible, secure configuration for RustAgent.

## Environment-Based Configuration

```rust
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub providers: Vec<ProviderConfig>,
    pub features: FeatureFlags,
    pub limits: LimitConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub workers: usize,
    pub request_timeout_secs: u64,
    pub shutdown_timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub path: String,
    pub pool_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub enabled: bool,
    pub api_key_env: Option<String>,
    pub base_url: String,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlags {
    pub enable_caching: bool,
    pub enable_streaming: bool,
    pub enable_compression: bool,
    pub enable_rate_limiting: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitConfig {
    pub max_concurrent_requests: usize,
    pub max_tokens_per_request: i64,
    pub cache_ttl_secs: u64,
    pub request_cache_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 20128,
                workers: 4,
                request_timeout_secs: 60,
                shutdown_timeout_secs: 30,
            },
            database: DatabaseConfig {
                path: "data/rust_agent.db".to_string(),
                pool_size: 5,
            },
            providers: vec![],
            features: FeatureFlags {
                enable_caching: true,
                enable_streaming: true,
                enable_compression: false,
                enable_rate_limiting: true,
            },
            limits: LimitConfig {
                max_concurrent_requests: 100,
                max_tokens_per_request: 4096,
                cache_ttl_secs: 3600,
                request_cache_size: 1000,
            },
        }
    }
}

impl Config {
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let mut config = Config::default();

        if let Ok(host) = env::var("RUST_AGENT_HOST") {
            config.server.host = host;
        }

        if let Ok(port) = env::var("RUST_AGENT_PORT") {
            config.server.port = port.parse()?;
        }

        if let Ok(workers) = env::var("RUST_AGENT_WORKERS") {
            config.server.workers = workers.parse()?;
        }

        if let Ok(db_path) = env::var("RUST_AGENT_DB_PATH") {
            config.database.path = db_path;
        }

        if let Ok(cache_enabled) = env::var("RUST_AGENT_ENABLE_CACHE") {
            config.features.enable_caching = cache_enabled.to_lowercase() == "true";
        }

        if let Ok(streaming_enabled) = env::var("RUST_AGENT_ENABLE_STREAMING") {
            config.features.enable_streaming = streaming_enabled.to_lowercase() == "true";
        }

        Ok(config)
    }

    pub fn from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config = toml::from_str(&content)?;
        Ok(config)
    }
}
```

## API Key Management

```rust
use std::collections::HashMap;
use once_cell::sync::Lazy;

pub struct ApiKeyManager {
    keys: HashMap<String, String>,
}

impl ApiKeyManager {
    pub fn from_env() -> Self {
        let mut keys = HashMap::new();

        // Load provider API keys from environment
        let providers = vec!["GROQ", "TOGETHER", "HUGGINGFACE", "OPENCODE", "POLLINATIONS"];

        for provider in providers {
            let env_var = format!("{}_API_KEY", provider);
            if let Ok(key) = std::env::var(&env_var) {
                keys.insert(provider.to_lowercase(), key);
            }
        }

        ApiKeyManager { keys }
    }

    pub fn get_key(&self, provider: &str) -> Option<String> {
        self.keys.get(provider).cloned()
    }

    pub fn has_key(&self, provider: &str) -> bool {
        self.keys.contains_key(provider)
    }

    pub fn set_key(&mut self, provider: &str, key: String) {
        self.keys.insert(provider.to_string(), key);
    }
}

static API_KEYS: Lazy<std::sync::Mutex<ApiKeyManager>> =
    Lazy::new(|| std::sync::Mutex::new(ApiKeyManager::from_env()));

pub fn get_api_key(provider: &str) -> Option<String> {
    API_KEYS.lock().ok().and_then(|mgr| mgr.get_key(provider))
}
```

## Feature Flags

```rust
use std::sync::Arc;
use dashmap::DashMap;

pub struct FeatureFlagManager {
    flags: Arc<DashMap<String, bool>>,
}

impl FeatureFlagManager {
    pub fn new(config: &FeatureFlags) -> Self {
        let flags = DashMap::new();

        flags.insert("cache".to_string(), config.enable_caching);
        flags.insert("streaming".to_string(), config.enable_streaming);
        flags.insert("compression".to_string(), config.enable_compression);
        flags.insert("rate_limiting".to_string(), config.enable_rate_limiting);

        FeatureFlagManager {
            flags: Arc::new(flags),
        }
    }

    pub fn is_enabled(&self, feature: &str) -> bool {
        self.flags
            .get(feature)
            .map(|v| *v)
            .unwrap_or(false)
    }

    pub fn set_flag(&self, feature: &str, enabled: bool) {
        self.flags.insert(feature.to_string(), enabled);
        tracing::info!("Feature flag {} set to {}", feature, enabled);
    }

    pub fn toggle_flag(&self, feature: &str) {
        let current = self.is_enabled(feature);
        self.set_flag(feature, !current);
    }

    pub fn get_all_flags(&self) -> HashMap<String, bool> {
        self.flags
            .iter()
            .map(|entry| (entry.key().clone(), *entry.value()))
            .collect()
    }
}
```

## Runtime Configuration Updates

```rust
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct RuntimeConfig {
    inner: Arc<RwLock<Config>>,
}

impl RuntimeConfig {
    pub fn new(config: Config) -> Self {
        RuntimeConfig {
            inner: Arc::new(RwLock::new(config)),
        }
    }

    pub async fn get(&self) -> Config {
        self.inner.read().await.clone()
    }

    pub async fn update(&self, updater: impl FnOnce(&mut Config)) {
        let mut config = self.inner.write().await;
        updater(&mut config);
        tracing::info!("Configuration updated");
    }

    pub async fn set_feature_flag(&self, feature: &str, enabled: bool) {
        let mut config = self.inner.write().await;
        match feature {
            "cache" => config.features.enable_caching = enabled,
            "streaming" => config.features.enable_streaming = enabled,
            "compression" => config.features.enable_compression = enabled,
            "rate_limiting" => config.features.enable_rate_limiting = enabled,
            _ => {}
        }
    }

    pub async fn set_limit(&self, limit: &str, value: usize) {
        let mut config = self.inner.write().await;
        match limit {
            "max_concurrent" => config.limits.max_concurrent_requests = value,
            "cache_size" => config.limits.request_cache_size = value,
            _ => {}
        }
    }
}
```

## Configuration Validation

```rust
pub struct ConfigValidator;

impl ConfigValidator {
    pub fn validate(config: &Config) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if config.server.port == 0 {
            errors.push("Server port must be > 0".to_string());
        }

        if config.server.workers == 0 {
            errors.push("Workers must be > 0".to_string());
        }

        if config.limits.max_concurrent_requests == 0 {
            errors.push("Max concurrent requests must be > 0".to_string());
        }

        if config.limits.max_tokens_per_request == 0 {
            errors.push("Max tokens per request must be > 0".to_string());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}
```

## Configuration Endpoints

```rust
pub async fn get_config(
    State(state): State<Arc<AppState>>,
) -> Json<Config> {
    Json(state.runtime_config.get().await)
}

pub async fn update_feature_flag(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UpdateFlagRequest>,
) -> Result<Json<serde_json::json!>, ApiError> {
    state.runtime_config
        .set_feature_flag(&payload.feature, payload.enabled)
        .await;

    Ok(Json(serde_json::json!({
        "status": "success",
        "message": format!("Feature {} set to {}", payload.feature, payload.enabled)
    })))
}

pub async fn update_limit(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UpdateLimitRequest>,
) -> Result<Json<serde_json::json!>, ApiError> {
    state.runtime_config
        .set_limit(&payload.limit, payload.value)
        .await;

    Ok(Json(serde_json::json!({
        "status": "success",
        "message": format!("Limit {} set to {}", payload.limit, payload.value)
    })))
}

#[derive(Deserialize)]
pub struct UpdateFlagRequest {
    pub feature: String,
    pub enabled: bool,
}

#[derive(Deserialize)]
pub struct UpdateLimitRequest {
    pub limit: String,
    pub value: usize,
}
```

## Example Configuration File

```toml
[server]
host = "127.0.0.1"
port = 20128
workers = 4
request_timeout_secs = 60
shutdown_timeout_secs = 30

[database]
path = "data/rust_agent.db"
pool_size = 5

[features]
enable_caching = true
enable_streaming = true
enable_compression = false
enable_rate_limiting = true

[limits]
max_concurrent_requests = 100
max_tokens_per_request = 4096
cache_ttl_secs = 3600
request_cache_size = 1000

[[providers]]
id = "opencode"
enabled = true
base_url = "https://api.opencode.ai/v1"
timeout_secs = 60

[[providers]]
id = "groq"
enabled = true
api_key_env = "GROQ_API_KEY"
base_url = "https://api.groq.com/v1"
timeout_secs = 60
```

## Environment Variables Reference

```bash
# Server
RUST_AGENT_HOST=127.0.0.1
RUST_AGENT_PORT=20128
RUST_AGENT_WORKERS=4

# Database
RUST_AGENT_DB_PATH=data/rust_agent.db

# Features
RUST_AGENT_ENABLE_CACHE=true
RUST_AGENT_ENABLE_STREAMING=true
RUST_AGENT_ENABLE_COMPRESSION=false
RUST_AGENT_ENABLE_RATE_LIMITING=true

# Provider API Keys
GROQ_API_KEY=<your-key>
TOGETHER_API_KEY=<your-key>
HUGGINGFACE_API_KEY=<your-key>

# Logging
RUST_LOG=rust_agent=debug,info
```
