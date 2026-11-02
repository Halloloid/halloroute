//! Admin endpoints for model pools (virtual load-balanced models).
use crate::errors::{ApiError, ApiResult};
use crate::pools::{
    store, validate_pool_id, ModelPool, PoolMember, PoolStrategy, MAX_POOL_MEMBERS, MAX_WEIGHT,
};
use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct PoolRequest {
    /// Required on create; ignored on update (taken from the URL)
    #[serde(default)]
    pub id: Option<String>,
    pub strategy: String,
    pub members: Vec<PoolMember>,
}

#[derive(Debug, Serialize)]
pub struct PoolsListResponse {
    pub pools: Vec<ModelPool>,
}

#[derive(Debug, Serialize)]
pub struct Candidate {
    pub provider_id: String,
    pub model: String,
    pub healthy: bool,
}

fn invalid(msg: impl Into<String>) -> ApiError {
    ApiError::InvalidRequest(msg.into())
}

/// Validate members against the live provider registry.
fn validate_members(state: &AppState, members: &[PoolMember]) -> ApiResult<()> {
    if members.is_empty() {
        return Err(invalid("A pool needs at least one member"));
    }
    if members.len() > MAX_POOL_MEMBERS {
        return Err(invalid(format!(
            "A pool can have at most {} members",
            MAX_POOL_MEMBERS
        )));
    }
    let mut seen = HashSet::new();
    for m in members {
        if m.weight < 1 || m.weight > MAX_WEIGHT {
            return Err(invalid(format!(
                "Weight for {}/{} must be between 1 and {}",
                m.provider_id, m.model, MAX_WEIGHT
            )));
        }
        let provider = state
            .providers
            .get(&m.provider_id)
            .ok_or_else(|| invalid(format!("Unknown provider '{}'", m.provider_id)))?;
        if !provider.models.contains(&m.model) {
            return Err(invalid(format!(
                "Provider '{}' does not serve model '{}'",
                m.provider_id, m.model
            )));
        }
        if !seen.insert((m.provider_id.clone(), m.model.clone())) {
            return Err(invalid(format!(
                "Duplicate member {}/{}",
                m.provider_id, m.model
            )));
        }
    }
    Ok(())
}

/// A pool name must not shadow a real model id, or requests would be ambiguous.
fn ensure_not_real_model(state: &AppState, id: &str) -> ApiResult<()> {
    let clash = state
        .providers
        .iter()
        .any(|e| e.value().models.iter().any(|m| m == id));
    if clash {
        Err(invalid(format!(
            "'{}' is already a real model id; choose a different pool name",
            id
        )))
    } else {
        Ok(())
    }
}

/// `GET /admin/pools`
pub async fn list_model_pools(State(state): State<Arc<AppState>>) -> Json<PoolsListResponse> {
    Json(PoolsListResponse {
        pools: state.pools.list(),
    })
}

/// `GET /admin/pools/candidates` — every provider model that can be added to a pool.
pub async fn pool_candidates(State(state): State<Arc<AppState>>) -> Json<Vec<Candidate>> {
    Json(pool_candidate_list(&state))
}

pub fn pool_candidate_list(state: &AppState) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = state
        .providers
        .iter()
        .flat_map(|e| {
            let p = e.value();
            p.models
                .iter()
                .map(|m| Candidate {
                    provider_id: p.id.clone(),
                    model: m.clone(),
                    healthy: p.is_available(),
                })
                .collect::<Vec<_>>()
        })
        .collect();
    out.sort_by(|a, b| (&a.provider_id, &a.model).cmp(&(&b.provider_id, &b.model)));
    out
}

/// `POST /admin/pools`
pub async fn create_model_pool(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PoolRequest>,
) -> ApiResult<(StatusCode, Json<ModelPool>)> {
    let id = req.id.unwrap_or_default().trim().to_string();
    validate_pool_id(&id)?;
    if state.pools.contains(&id) {
        return Err(invalid(format!("A pool named '{}' already exists", id)));
    }
    ensure_not_real_model(&state, &id)?;
    let strategy = PoolStrategy::parse(&req.strategy)?;
    validate_members(&state, &req.members)?;

    let pool = ModelPool {
        id,
        strategy,
        members: req.members,
    };
    store::save_pool(&state.db_pool, &pool).await?;
    state.pools.insert(pool.clone());
    tracing::info!(pool = %pool.id, members = pool.members.len(), strategy = pool.strategy.as_str(), "Model pool created");
    Ok((StatusCode::CREATED, Json(pool)))
}

/// `PUT /admin/pools/:id`
pub async fn update_model_pool(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<PoolRequest>,
) -> ApiResult<Json<ModelPool>> {
    if !state.pools.contains(&id) {
        return Err(ApiError::NotFound(format!("Pool '{}' not found", id)));
    }
    let strategy = PoolStrategy::parse(&req.strategy)?;
    validate_members(&state, &req.members)?;

    let pool = ModelPool {
        id,
        strategy,
        members: req.members,
    };
    store::save_pool(&state.db_pool, &pool).await?;
    state.pools.insert(pool.clone());
    tracing::info!(pool = %pool.id, "Model pool updated");
    Ok(Json(pool))
}

/// `DELETE /admin/pools/:id`
pub async fn delete_model_pool(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let removed = store::delete_pool(&state.db_pool, &id).await?;
    let in_memory = state.pools.remove(&id);
    if removed || in_memory {
        tracing::info!(pool = %id, "Model pool deleted");
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound(format!("Pool '{}' not found", id)))
    }
}
