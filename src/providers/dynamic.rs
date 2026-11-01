/// Dynamic provider management - runtime CRUD for user-added providers
use super::types::{FreeModelFilter, ModelInfo, Provider};
use crate::errors::{ApiError, ApiResult};
use crate::quota::DbPool;
use chrono::Local;
use rusqlite::params;
use serde::{Deserialize, Serialize};

/// User-submitted provider configuration for dynamic registration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicProviderConfig {
    pub id: String,
    pub name: String,
    pub api_endpoint: String,
    pub auth_type: String, // "none", "api_key", "bearer"
    pub api_key: Option<String>,
    pub free_quota_daily: Option<i64>,
    pub free_quota_monthly: Option<i64>,
    pub rate_limit_rpm: i32,
    pub free_model_filter: String, // Serialized FreeModelFilter
}

/// Persisted dynamic provider record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicProviderRecord {
    pub id: String,
    pub name: String,
    pub api_endpoint: String,
    pub auth_type: String,
    /// Never serialized into API responses.
    #[serde(skip_serializing, default)]
    pub api_key: Option<String>,
    pub free_quota_daily: Option<i64>,
    pub free_quota_monthly: Option<i64>,
    pub rate_limit_rpm: i32,
    pub models: Vec<String>,
    pub free_model_filter: String,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Ping response with discovered models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingResponse {
    pub success: bool,
    pub models: Vec<ModelInfo>,
    pub error: Option<String>,
}

/// Parse FreeModelFilter from string representation
pub fn parse_free_model_filter(filter: &str) -> ApiResult<FreeModelFilter> {
    match filter {
        "all" => Ok(FreeModelFilter::All),
        "none" => Ok(FreeModelFilter::None),
        s if s.starts_with("patterns:") => {
            let patterns = s
                .strip_prefix("patterns:")
                .unwrap()
                .split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect();
            Ok(FreeModelFilter::Patterns(patterns))
        }
        s if s.starts_with("explicit:") => {
            let models = s
                .strip_prefix("explicit:")
                .unwrap()
                .split(',')
                .map(|m| m.trim().to_string())
                .filter(|m| !m.is_empty())
                .collect();
            Ok(FreeModelFilter::ExplicitList(models))
        }
        _ => Err(ApiError::InvalidRequest(format!(
            "Invalid free_model_filter format: {}. Expected 'all', 'none', 'patterns:...', or 'explicit:...'",
            filter
        ))),
    }
}

/// Serialize FreeModelFilter to string
pub fn serialize_free_model_filter(filter: &FreeModelFilter) -> String {
    match filter {
        FreeModelFilter::All => "all".to_string(),
        FreeModelFilter::None => "none".to_string(),
        FreeModelFilter::Patterns(patterns) => format!("patterns:{}", patterns.join(",")),
        FreeModelFilter::ExplicitList(models) => format!("explicit:{}", models.join(",")),
        FreeModelFilter::PricingAPI { .. } => "pricing_api".to_string(),
    }
}

/// Create a new dynamic provider
pub async fn create_dynamic_provider(
    pool: &DbPool,
    config: DynamicProviderConfig,
    discovered_models: Vec<String>,
) -> ApiResult<DynamicProviderRecord> {
    let guard = pool.get_connection().lock().await;
    let conn = guard
        .as_ref()
        .ok_or_else(|| ApiError::DatabaseError("No database connection".into()))?;

    let now = Local::now().to_rfc3339();
    let models_json = serde_json::to_string(&discovered_models)
        .map_err(|e| ApiError::InternalError(format!("Failed to serialize models: {}", e)))?;

    conn.execute(
        "INSERT INTO dynamic_providers
         (id, name, api_endpoint, auth_type, api_key, free_quota_daily, free_quota_monthly,
          rate_limit_rpm, models, free_model_filter, is_active, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, ?11, ?11)",
        params![
            &config.id,
            &config.name,
            &config.api_endpoint,
            &config.auth_type,
            &config.api_key,
            config.free_quota_daily,
            config.free_quota_monthly,
            config.rate_limit_rpm,
            &models_json,
            &config.free_model_filter,
            &now,
        ],
    )
    .map_err(|e| ApiError::DatabaseError(format!("Failed to insert dynamic provider: {}", e)))?;

    Ok(DynamicProviderRecord {
        id: config.id,
        name: config.name,
        api_endpoint: config.api_endpoint,
        auth_type: config.auth_type,
        api_key: config.api_key,
        free_quota_daily: config.free_quota_daily,
        free_quota_monthly: config.free_quota_monthly,
        rate_limit_rpm: config.rate_limit_rpm,
        models: discovered_models,
        free_model_filter: config.free_model_filter,
        is_active: true,
        created_at: now.clone(),
        updated_at: now,
    })
}

/// List all dynamic providers
pub async fn list_dynamic_providers(pool: &DbPool) -> ApiResult<Vec<DynamicProviderRecord>> {
    let guard = pool.get_connection().lock().await;
    let conn = guard
        .as_ref()
        .ok_or_else(|| ApiError::DatabaseError("No database connection".into()))?;

    let mut stmt = conn
        .prepare(
            "SELECT id, name, api_endpoint, auth_type, api_key, free_quota_daily,
                    free_quota_monthly, rate_limit_rpm, models, free_model_filter,
                    is_active, created_at, updated_at
             FROM dynamic_providers
             ORDER BY created_at DESC",
        )
        .map_err(|e| ApiError::DatabaseError(e.to_string()))?;

    let rows = stmt
        .query_map([], |row| {
            let models_json: String = row.get(8)?;
            let models: Vec<String> = serde_json::from_str(&models_json).unwrap_or_default();
            let is_active: i32 = row.get(10)?;

            Ok(DynamicProviderRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                api_endpoint: row.get(2)?,
                auth_type: row.get(3)?,
                api_key: row.get(4)?,
                free_quota_daily: row.get(5)?,
                free_quota_monthly: row.get(6)?,
                rate_limit_rpm: row.get(7)?,
                models,
                free_model_filter: row.get(9)?,
                is_active: is_active != 0,
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
            })
        })
        .map_err(|e| ApiError::DatabaseError(e.to_string()))?;

    let mut providers = Vec::new();
    for row in rows.flatten() {
        providers.push(row);
    }

    Ok(providers)
}

/// Get a specific dynamic provider by ID
pub async fn get_dynamic_provider(pool: &DbPool, id: &str) -> ApiResult<DynamicProviderRecord> {
    let guard = pool.get_connection().lock().await;
    let conn = guard
        .as_ref()
        .ok_or_else(|| ApiError::DatabaseError("No database connection".into()))?;

    let mut stmt = conn
        .prepare(
            "SELECT id, name, api_endpoint, auth_type, api_key, free_quota_daily,
                    free_quota_monthly, rate_limit_rpm, models, free_model_filter,
                    is_active, created_at, updated_at
             FROM dynamic_providers
             WHERE id = ?",
        )
        .map_err(|e| ApiError::DatabaseError(e.to_string()))?;

    let result = stmt.query_row(params![id], |row| {
        let models_json: String = row.get(8)?;
        let models: Vec<String> = serde_json::from_str(&models_json).unwrap_or_default();
        let is_active: i32 = row.get(10)?;

        Ok(DynamicProviderRecord {
            id: row.get(0)?,
            name: row.get(1)?,
            api_endpoint: row.get(2)?,
            auth_type: row.get(3)?,
            api_key: row.get(4)?,
            free_quota_daily: row.get(5)?,
            free_quota_monthly: row.get(6)?,
            rate_limit_rpm: row.get(7)?,
            models,
            free_model_filter: row.get(9)?,
            is_active: is_active != 0,
            created_at: row.get(11)?,
            updated_at: row.get(12)?,
        })
    });

    match result {
        Ok(record) => Ok(record),
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            Err(ApiError::ProviderNotFound(id.to_string()))
        }
        Err(e) => Err(ApiError::DatabaseError(e.to_string())),
    }
}

/// Update a dynamic provider
pub async fn update_dynamic_provider(
    pool: &DbPool,
    id: &str,
    config: DynamicProviderConfig,
    discovered_models: Vec<String>,
) -> ApiResult<DynamicProviderRecord> {
    let guard = pool.get_connection().lock().await;
    let conn = guard
        .as_ref()
        .ok_or_else(|| ApiError::DatabaseError("No database connection".into()))?;

    let now = Local::now().to_rfc3339();
    let models_json = serde_json::to_string(&discovered_models)
        .map_err(|e| ApiError::InternalError(format!("Failed to serialize models: {}", e)))?;

    let rows_affected = conn
        .execute(
            "UPDATE dynamic_providers
             SET name = ?1, api_endpoint = ?2, auth_type = ?3, api_key = ?4,
                 free_quota_daily = ?5, free_quota_monthly = ?6, rate_limit_rpm = ?7,
                 models = ?8, free_model_filter = ?9, updated_at = ?10
             WHERE id = ?11",
            params![
                &config.name,
                &config.api_endpoint,
                &config.auth_type,
                &config.api_key,
                config.free_quota_daily,
                config.free_quota_monthly,
                config.rate_limit_rpm,
                &models_json,
                &config.free_model_filter,
                &now,
                id,
            ],
        )
        .map_err(|e| {
            ApiError::DatabaseError(format!("Failed to update dynamic provider: {}", e))
        })?;

    if rows_affected == 0 {
        return Err(ApiError::ProviderNotFound(id.to_string()));
    }

    Ok(DynamicProviderRecord {
        id: id.to_string(),
        name: config.name,
        api_endpoint: config.api_endpoint,
        auth_type: config.auth_type,
        api_key: config.api_key,
        free_quota_daily: config.free_quota_daily,
        free_quota_monthly: config.free_quota_monthly,
        rate_limit_rpm: config.rate_limit_rpm,
        models: discovered_models,
        free_model_filter: config.free_model_filter,
        is_active: true,
        created_at: String::new(), // Not modified
        updated_at: now,
    })
}

/// Delete a dynamic provider
pub async fn delete_dynamic_provider(pool: &DbPool, id: &str) -> ApiResult<()> {
    let guard = pool.get_connection().lock().await;
    let conn = guard
        .as_ref()
        .ok_or_else(|| ApiError::DatabaseError("No database connection".into()))?;

    let rows_affected = conn
        .execute("DELETE FROM dynamic_providers WHERE id = ?", params![id])
        .map_err(|e| {
            ApiError::DatabaseError(format!("Failed to delete dynamic provider: {}", e))
        })?;

    if rows_affected == 0 {
        return Err(ApiError::ProviderNotFound(id.to_string()));
    }

    Ok(())
}

/// Convert DynamicProviderRecord to Provider (for runtime registration)
pub fn record_to_provider(record: &DynamicProviderRecord) -> Provider {
    let mut provider = Provider::new(
        record.id.clone(),
        record.name.clone(),
        record.api_endpoint.clone(),
        record.auth_type.clone(),
        record.free_quota_daily,
        record.free_quota_monthly,
        record.rate_limit_rpm,
        record.models.clone(),
    );
    provider.api_key = record.api_key.clone();
    provider
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_free_model_filter() {
        let filter = parse_free_model_filter("all").unwrap();
        assert!(matches!(filter, FreeModelFilter::All));

        let filter = parse_free_model_filter("none").unwrap();
        assert!(matches!(filter, FreeModelFilter::None));

        let filter = parse_free_model_filter("patterns:llama*,gpt*").unwrap();
        if let FreeModelFilter::Patterns(patterns) = filter {
            assert_eq!(patterns.len(), 2);
            assert!(patterns.contains(&"llama*".to_string()));
        } else {
            panic!("Expected Patterns variant");
        }

        let filter = parse_free_model_filter("explicit:model1,model2").unwrap();
        if let FreeModelFilter::ExplicitList(models) = filter {
            assert_eq!(models.len(), 2);
        } else {
            panic!("Expected ExplicitList variant");
        }
    }

    #[test]
    fn test_serialize_free_model_filter() {
        assert_eq!(serialize_free_model_filter(&FreeModelFilter::All), "all");
        assert_eq!(serialize_free_model_filter(&FreeModelFilter::None), "none");

        let filter = FreeModelFilter::Patterns(vec!["llama*".into(), "gpt*".into()]);
        assert_eq!(serialize_free_model_filter(&filter), "patterns:llama*,gpt*");

        let filter = FreeModelFilter::ExplicitList(vec!["model1".into(), "model2".into()]);
        assert_eq!(
            serialize_free_model_filter(&filter),
            "explicit:model1,model2"
        );
    }
}
