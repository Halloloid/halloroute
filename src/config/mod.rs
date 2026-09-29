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
    pub fn from_env() -> Self {
        let mut config = Config::default();

        if let Ok(host) = env::var("RUST_AGENT_HOST").or_else(|_| env::var("HOST")) {
            config.server.host = host;
        }

        if let Ok(port) = env::var("RUST_AGENT_PORT").or_else(|_| env::var("PORT")) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn test_config_defaults() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let config = Config::default();
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.port, 20128);
        assert_eq!(config.server.workers, 4);
        assert_eq!(config.database.path, "data/rust_agent.db");
    }

    #[test]
    fn test_config_from_env() {
        let _lock = ENV_MUTEX.lock().unwrap();
        std::env::set_var("RUST_AGENT_HOST", "0.0.0.0");
        std::env::set_var("RUST_AGENT_PORT", "9090");
        std::env::set_var("RUST_AGENT_DB_PATH", "/tmp/test.db");

        let config = Config::from_env();
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.server.port, 9090);
        assert_eq!(config.database.path, "/tmp/test.db");

        std::env::remove_var("RUST_AGENT_HOST");
        std::env::remove_var("RUST_AGENT_PORT");
        std::env::remove_var("RUST_AGENT_DB_PATH");
    }

    #[test]
    fn test_config_from_env_invalid_port_fallback() {
        let _lock = ENV_MUTEX.lock().unwrap();
        std::env::set_var("RUST_AGENT_PORT", "not_a_number");

        let config = Config::from_env();
        assert_eq!(config.server.port, 20128);

        std::env::remove_var("RUST_AGENT_PORT");
    }
}
