use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub api_endpoint: String,
    pub auth_type: String, // "none", "api_key", "bearer"
    pub free_quota_daily: Option<i64>,
    pub free_quota_monthly: Option<i64>,
    pub rate_limit_rpm: i32,
    pub models: Vec<String>,
    pub health_status: ProviderHealth,
    pub last_check: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderHealth {
    Healthy,
    Degraded,
    Down,
}

impl Provider {
    pub fn new(
        id: String,
        name: String,
        api_endpoint: String,
        auth_type: String,
        free_quota_daily: Option<i64>,
        free_quota_monthly: Option<i64>,
        rate_limit_rpm: i32,
        models: Vec<String>,
    ) -> Self {
        Provider {
            id,
            name,
            api_endpoint,
            auth_type,
            free_quota_daily,
            free_quota_monthly,
            rate_limit_rpm,
            models,
            health_status: ProviderHealth::Healthy,
            last_check: None,
        }
    }

    pub fn is_available(&self) -> bool {
        self.health_status == ProviderHealth::Healthy
    }
}
