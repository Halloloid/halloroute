use serde::{Deserialize, Serialize};
use std::env;

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub workers: usize,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub path: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 20128,
                workers: 4,
            },
            database: DatabaseConfig {
                path: "data/rust_agent.db".to_string(),
            },
        }
    }
}

impl Config {
    #[allow(dead_code)]
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
