use actix_web::{web, HttpResponse};
use serde_json::json;

use crate::errors::ApiResult;
use crate::server::AppState;

pub async fn get_providers(state: web::Data<AppState>) -> ApiResult<HttpResponse> {
    let registry = crate::providers::ProviderRegistry::new(state.providers.clone());
    let providers = registry.list_providers();

    let provider_list: Vec<serde_json::Value> = providers
        .into_iter()
        .map(|p| {
            json!({
                "id": p.id,
                "name": p.name,
                "health_status": p.health_status,
                "rate_limit_rpm": p.rate_limit_rpm,
                "models": p.models,
                "free_quota_daily": p.free_quota_daily,
                "free_quota_monthly": p.free_quota_monthly,
            })
        })
        .collect();

    Ok(HttpResponse::Ok().json(json!({
        "providers": provider_list,
        "count": provider_list.len(),
    })))
}

pub async fn get_quota_status(state: web::Data<AppState>) -> ApiResult<HttpResponse> {
    let registry = crate::providers::ProviderRegistry::new(state.providers.clone());
    let providers = registry.list_providers();

    let mut quota_status = Vec::new();

    for provider in providers {
        if let Ok(current_usage) =
            crate::db::get_today_quota_usage(&state.db_pool, &provider.id).await
        {
            let remaining = provider
                .free_quota_daily
                .map(|limit| limit - current_usage)
                .unwrap_or(-1);

            quota_status.push(json!({
                "provider_id": provider.id,
                "provider_name": provider.name,
                "daily_limit": provider.free_quota_daily,
                "current_usage": current_usage,
                "remaining": remaining,
                "percentage_used": if let Some(limit) = provider.free_quota_daily {
                    (current_usage as f64 / limit as f64 * 100.0).round() / 100.0
                } else {
                    -1.0
                },
            }));
        }
    }

    Ok(HttpResponse::Ok().json(json!({
        "quota_status": quota_status,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    })))
}

pub async fn disable_provider(
    _state: web::Data<AppState>,
    provider_id: web::Path<String>,
) -> ApiResult<HttpResponse> {
    tracing::info!(provider_id = %provider_id, "Disabling provider");

    Ok(HttpResponse::Ok().json(json!({
        "message": format!("Provider {} disabled", provider_id),
    })))
}

pub async fn enable_provider(
    _state: web::Data<AppState>,
    provider_id: web::Path<String>,
) -> ApiResult<HttpResponse> {
    tracing::info!(provider_id = %provider_id, "Enabling provider");

    Ok(HttpResponse::Ok().json(json!({
        "message": format!("Provider {} enabled", provider_id),
    })))
}
