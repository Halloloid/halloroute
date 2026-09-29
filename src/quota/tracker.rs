use chrono::Local;
use dashmap::DashMap;
use rusqlite::{params, Connection};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::errors::{ApiError, ApiResult};

/// Database connection and in-memory cache pool
#[derive(Clone)]
pub struct DbPool {
    // Fast-path in-memory cache: "provider_id:YYYY-MM-DD" -> tokens_used
    in_memory: Arc<DashMap<String, i64>>,
    // SQLite persistent connection with WAL mode
    sqlite: Arc<Mutex<Option<Connection>>>,
    #[allow(dead_code)]
    db_path: String,
}

impl DbPool {
    pub fn in_memory_only() -> Self {
        DbPool {
            in_memory: Arc::new(DashMap::new()),
            sqlite: Arc::new(Mutex::new(None)),
            db_path: ":memory:".to_string(),
        }
    }
}

pub async fn init_pool() -> ApiResult<DbPool> {
    let db_path =
        std::env::var("RUSTAGENT_DB_PATH").unwrap_or_else(|_| "rust_agent.db".to_string());
    tracing::info!(db_path = %db_path, "Initializing SQLite quota database");

    let conn = Connection::open(&db_path).map_err(|e| {
        ApiError::DatabaseError(format!(
            "Failed to open SQLite database at {}: {}",
            db_path, e
        ))
    })?;

    // Optimize SQLite for high concurrency / low memory
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA temp_store = MEMORY;",
    )
    .map_err(|e| ApiError::DatabaseError(format!("Failed to set SQLite pragmas: {}", e)))?;

    Ok(DbPool {
        in_memory: Arc::new(DashMap::new()),
        sqlite: Arc::new(Mutex::new(Some(conn))),
        db_path,
    })
}

pub async fn run_migrations(pool: &DbPool) -> ApiResult<()> {
    let guard = pool.sqlite.lock().await;
    if let Some(conn) = guard.as_ref() {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS quota_usage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider_id TEXT NOT NULL,
                period_key TEXT NOT NULL,
                tokens_used INTEGER NOT NULL DEFAULT 0,
                requests_count INTEGER NOT NULL DEFAULT 0,
                last_updated TEXT NOT NULL,
                UNIQUE(provider_id, period_key)
            );
            CREATE INDEX IF NOT EXISTS idx_quota_provider_period ON quota_usage(provider_id, period_key);

            CREATE TABLE IF NOT EXISTS request_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider_id TEXT NOT NULL,
                model TEXT NOT NULL,
                prompt_tokens INTEGER NOT NULL,
                completion_tokens INTEGER NOT NULL,
                total_tokens INTEGER NOT NULL,
                latency_ms INTEGER NOT NULL,
                status_code INTEGER NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_logs_created_at ON request_logs(created_at);",
        )
        .map_err(|e| ApiError::DatabaseError(format!("Failed to run SQLite migrations: {}", e)))?;

        // Load existing today's tokens into in-memory cache
        let today = Local::now().format("%Y-%m-%d").to_string();
        let mut stmt = conn
            .prepare("SELECT provider_id, period_key, tokens_used FROM quota_usage WHERE period_key LIKE ?")
            .map_err(|e| ApiError::DatabaseError(e.to_string()))?;

        let rows = stmt
            .query_map(params![format!("%:{}", today)], |row| {
                Ok((row.get::<_, String>(1)?, row.get::<_, i64>(2)?))
            })
            .map_err(|e| ApiError::DatabaseError(e.to_string()))?;

        for row in rows.flatten() {
            pool.in_memory.insert(row.0, row.1);
        }

        tracing::info!("SQLite migrations complete and today's quotas loaded");
    }
    Ok(())
}

pub async fn get_today_quota_usage(pool: &DbPool, provider_id: &str) -> ApiResult<i64> {
    let today = Local::now().format("%Y-%m-%d").to_string();
    let key = format!("{}:{}", provider_id, today);

    // Fast-path: Check in-memory cache
    if let Some(cached) = pool.in_memory.get(&key) {
        return Ok(*cached);
    }

    // SQLite fallback lookup
    let guard = pool.sqlite.lock().await;
    if let Some(conn) = guard.as_ref() {
        let mut stmt = conn
            .prepare("SELECT tokens_used FROM quota_usage WHERE provider_id = ? AND period_key = ?")
            .map_err(|e| ApiError::DatabaseError(e.to_string()))?;

        let result = stmt.query_row(params![provider_id, &key], |row| row.get::<_, i64>(0));
        match result {
            Ok(tokens) => {
                pool.in_memory.insert(key, tokens);
                Ok(tokens)
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                pool.in_memory.insert(key, 0);
                Ok(0)
            }
            Err(e) => Err(ApiError::DatabaseError(e.to_string())),
        }
    } else {
        Ok(0)
    }
}

pub async fn increment_quota_usage(pool: &DbPool, provider_id: &str, tokens: i64) -> ApiResult<()> {
    let now = Local::now();
    let today = now.format("%Y-%m-%d").to_string();
    let key = format!("{}:{}", provider_id, today);
    let timestamp = now.to_rfc3339();

    // 1. Update fast-path in-memory cache
    pool.in_memory
        .entry(key.clone())
        .and_modify(|v| *v += tokens)
        .or_insert(tokens);

    // 2. Persist to SQLite
    let guard = pool.sqlite.lock().await;
    if let Some(conn) = guard.as_ref() {
        conn.execute(
            "INSERT INTO quota_usage (provider_id, period_key, tokens_used, requests_count, last_updated)
             VALUES (?1, ?2, ?3, 1, ?4)
             ON CONFLICT(provider_id, period_key) DO UPDATE SET
                tokens_used = tokens_used + ?3,
                requests_count = requests_count + 1,
                last_updated = ?4",
            params![provider_id, key, tokens, timestamp],
        )
        .map_err(|e| ApiError::DatabaseError(format!("Failed to increment quota in SQLite: {}", e)))?;
    }

    Ok(())
}

/// Start background quota reset jobs
pub fn start_quota_reset_tasks(pool: DbPool) {
    tokio::spawn(async move {
        loop {
            let now = Local::now();
            let next_midnight = (now + chrono::Duration::days(1))
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .unwrap();
            let duration = (next_midnight - now.naive_local())
                .to_std()
                .unwrap_or(std::time::Duration::from_secs(3600));

            tokio::time::sleep(duration).await;
            tracing::info!("Daily quota reset timer triggered: clearing cache for new day");
            pool.in_memory.clear();
        }
    });
}
