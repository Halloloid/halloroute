//! SQLite persistence for model pools.
use super::{ModelPool, PoolMember, PoolStrategy};
use crate::errors::{ApiError, ApiResult};
use crate::quota::DbPool;
use chrono::Local;
use rusqlite::params;

pub const MODEL_POOLS_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS model_pools (
    id TEXT PRIMARY KEY,
    strategy TEXT NOT NULL,
    members TEXT NOT NULL,
    collective_enabled INTEGER NOT NULL DEFAULT 0,
    default_mode TEXT NOT NULL DEFAULT 'parallel',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);";

pub fn ensure_collective_column(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    let cols: Vec<String> = conn
        .prepare("PRAGMA table_info(model_pools)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(|r| r.ok())
        .collect();

    if !cols.iter().any(|c| c == "collective_enabled") {
        conn.execute(
            "ALTER TABLE model_pools ADD COLUMN collective_enabled INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    if !cols.iter().any(|c| c == "default_mode") {
        conn.execute(
            "ALTER TABLE model_pools ADD COLUMN default_mode TEXT NOT NULL DEFAULT 'parallel'",
            [],
        )?;
    }
    Ok(())
}

fn db_err(what: &str, e: impl std::fmt::Display) -> ApiError {
    ApiError::DatabaseError(format!("{}: {}", what, e))
}

/// Insert a pool or replace an existing one (keeps `created_at`).
pub async fn save_pool(db: &DbPool, pool: &ModelPool) -> ApiResult<()> {
    let members = serde_json::to_string(&pool.members)
        .map_err(|e| ApiError::InternalError(format!("Failed to serialize members: {}", e)))?;
    let now = Local::now().to_rfc3339();
    let mode_str = match pool.default_mode {
        crate::collective::CollectiveMode::Gan => "gan",
        crate::collective::CollectiveMode::Parallel => "parallel",
    };

    let guard = db.get_connection().lock().await;
    let conn = guard
        .as_ref()
        .ok_or_else(|| ApiError::DatabaseError("No database connection".into()))?;
    conn.execute(
        "INSERT INTO model_pools (id, strategy, members, collective_enabled, default_mode, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
         ON CONFLICT(id) DO UPDATE SET
            strategy           = excluded.strategy,
            members            = excluded.members,
            collective_enabled = excluded.collective_enabled,
            default_mode       = excluded.default_mode,
            updated_at         = excluded.updated_at",
        params![&pool.id, pool.strategy.as_str(), &members, pool.collective_enabled, mode_str, &now],
    )
    .map_err(|e| db_err("Failed to save pool", e))?;
    Ok(())
}

/// All stored pools. Rows that no longer parse are skipped with a warning.
pub async fn list_pools(db: &DbPool) -> ApiResult<Vec<ModelPool>> {
    let guard = db.get_connection().lock().await;
    let Some(conn) = guard.as_ref() else {
        return Ok(Vec::new());
    };
    let mut stmt = conn
        .prepare("SELECT id, strategy, members, collective_enabled, default_mode FROM model_pools ORDER BY id")
        .map_err(|e| db_err("Failed to list pools", e))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, bool>(3)?,
                row.get::<_, String>(4)
                    .unwrap_or_else(|_| "parallel".into()),
            ))
        })
        .map_err(|e| db_err("Failed to query pools", e))?;

    let mut pools = Vec::new();
    for row in rows {
        let (id, strategy, members, collective_enabled, default_mode_str) =
            row.map_err(|e| db_err("Failed to read pool row", e))?;
        let default_mode = if default_mode_str == "gan" {
            crate::collective::CollectiveMode::Gan
        } else {
            crate::collective::CollectiveMode::Parallel
        };
        let parsed = PoolStrategy::parse(&strategy).and_then(|s| {
            serde_json::from_str::<Vec<PoolMember>>(&members)
                .map(|m| (s, m))
                .map_err(|e| ApiError::InternalError(e.to_string()))
        });
        match parsed {
            Ok((strategy, members)) => pools.push(ModelPool {
                id,
                strategy,
                members,
                collective_enabled,
                default_mode,
            }),
            Err(e) => tracing::warn!(pool = %id, error = %e, "Skipping unreadable pool row"),
        }
    }
    Ok(pools)
}

/// Delete a pool. Returns whether a row was removed.
pub async fn delete_pool(db: &DbPool, id: &str) -> ApiResult<bool> {
    let guard = db.get_connection().lock().await;
    let conn = guard
        .as_ref()
        .ok_or_else(|| ApiError::DatabaseError("No database connection".into()))?;
    let n = conn
        .execute("DELETE FROM model_pools WHERE id = ?1", params![id])
        .map_err(|e| db_err("Failed to delete pool", e))?;
    Ok(n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quota::tracker::create_memory_pool;

    fn pool(id: &str, strategy: PoolStrategy) -> ModelPool {
        ModelPool {
            id: id.into(),
            strategy,
            members: vec![
                PoolMember {
                    provider_id: "a".into(),
                    model: "m1".into(),
                    weight: 3,
                },
                PoolMember {
                    provider_id: "b".into(),
                    model: "m2".into(),
                    weight: 1,
                },
            ],
            collective_enabled: false,
            default_mode: crate::collective::CollectiveMode::Parallel,
        }
    }

    #[tokio::test]
    async fn save_list_update_delete_roundtrip() {
        let db = create_memory_pool().unwrap();
        save_pool(&db, &pool("p1", PoolStrategy::Priority))
            .await
            .unwrap();
        save_pool(&db, &pool("p0", PoolStrategy::Lru))
            .await
            .unwrap();

        let listed = list_pools(&db).await.unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, "p0");
        assert_eq!(listed[1].members[0].weight, 3);
        assert_eq!(listed[1].members[1].provider_id, "b");

        save_pool(&db, &pool("p1", PoolStrategy::RoundRobin))
            .await
            .unwrap();
        let listed = list_pools(&db).await.unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[1].strategy, PoolStrategy::RoundRobin);

        assert!(delete_pool(&db, "p1").await.unwrap());
        assert!(!delete_pool(&db, "p1").await.unwrap());
        assert_eq!(list_pools(&db).await.unwrap().len(), 1);
    }
}
