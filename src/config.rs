use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub providers: Vec<ProviderConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub worker_threads: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub api_endpoint: String,
    pub auth_type: String, // "none", "api_key", "bearer"
    pub free_quota_daily: Option<i64>,
    pub free_quota_monthly: Option<i64>,
    pub rate_limit_rpm: i32,
    pub models: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 20128,
                worker_threads: 4,
            },
            database: DatabaseConfig {
                path: "data/rust_agent.db".to_string(),
            },
            providers: vec![],
        }
    }
}

impl Config {
    pub fn from_env() -> Self {
        let mut config = Config::default();

        if let Ok(host) = env::var("RUST_AGENT_HOST") {
            config.server.host = host;
        }

        if let Ok(port) = env::var("RUST_AGENT_PORT") {
            if let Ok(p) = port.parse() {
                config.server.port = p;
            }
        }

        if let Ok(db_path) = env::var("RUST_AGENT_DB_PATH") {
            config.database.path = db_path;
        }

        config
    }
}
