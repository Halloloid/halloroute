---
name: quota-tracking-patterns
description: Quota tracking and management patterns for free-tier providers, daily/monthly reset logic, race-free updates, and alert thresholds
used_by: RustAgent Gateway
---

# Quota Tracking Patterns

Implements accurate quota tracking for free-tier AI providers with daily/monthly resets and race-free concurrent updates.

## Quota Data Structure

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaEntry {
    pub provider_id: String,
    pub date: String,           // YYYY-MM-DD
    pub tokens_used: i64,
    pub requests_count: i32,
    pub reset_type: ResetType,  // Daily, Weekly, Monthly
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResetType {
    Daily,
    Weekly,
    Monthly,
}

#[derive(Debug, Clone)]
pub struct ProviderQuota {
    pub provider_id: String,
    pub daily_limit: Option<i64>,
    pub monthly_limit: Option<i64>,
    pub weekly_limit: Option<i64>,
    pub requests_per_minute: i32,
}
```

## In-Memory Quota Tracker

```rust
use dashmap::DashMap;
use chrono::Local;
use std::sync::Arc;

pub struct QuotaTracker {
    // Key: "provider_id:YYYY-MM-DD"
    daily_usage: Arc<DashMap<String, i64>>,
    // Key: "provider_id:YYYY-W##"
    weekly_usage: Arc<DashMap<String, i64>>,
    // Key: "provider_id:YYYY-MM"
    monthly_usage: Arc<DashMap<String, i64>>,
}

impl QuotaTracker {
    pub fn new() -> Self {
        QuotaTracker {
            daily_usage: Arc::new(DashMap::new()),
            weekly_usage: Arc::new(DashMap::new()),
            monthly_usage: Arc::new(DashMap::new()),
        }
    }

    pub fn get_today_key(provider_id: &str) -> String {
        let today = Local::now().format("%Y-%m-%d").to_string();
        format!("{}:{}", provider_id, today)
    }

    pub fn get_week_key(provider_id: &str) -> String {
        let week = Local::now().format("%Y-W%V").to_string();
        format!("{}:{}", provider_id, week)
    }

    pub fn get_month_key(provider_id: &str) -> String {
        let month = Local::now().format("%Y-%m").to_string();
        format!("{}:{}", provider_id, month)
    }

    pub async fn add_tokens(&self, provider_id: &str, tokens: i64) {
        let today_key = Self::get_today_key(provider_id);
        let week_key = Self::get_week_key(provider_id);
        let month_key = Self::get_month_key(provider_id);

        self.daily_usage
            .entry(today_key)
            .and_modify(|v| *v += tokens)
            .or_insert(tokens);

        self.weekly_usage
            .entry(week_key)
            .and_modify(|v| *v += tokens)
            .or_insert(tokens);

        self.monthly_usage
            .entry(month_key)
            .and_modify(|v| *v += tokens)
            .or_insert(tokens);
    }

    pub async fn get_daily_usage(&self, provider_id: &str) -> i64 {
        let key = Self::get_today_key(provider_id);
        self.daily_usage.get(&key).map(|v| *v).unwrap_or(0)
    }

    pub async fn get_monthly_usage(&self, provider_id: &str) -> i64 {
        let key = Self::get_month_key(provider_id);
        self.monthly_usage.get(&key).map(|v| *v).unwrap_or(0)
    }

    pub async fn check_quota(
        &self,
        provider_id: &str,
        quota: &ProviderQuota,
        requested_tokens: i64,
    ) -> Result<bool, QuotaError> {
        let daily_usage = self.get_daily_usage(provider_id).await;
        let monthly_usage = self.get_monthly_usage(provider_id).await;

        // Check daily quota
        if let Some(daily_limit) = quota.daily_limit {
            if daily_usage + requested_tokens > daily_limit {
                return Ok(false);
            }
        }

        // Check monthly quota
        if let Some(monthly_limit) = quota.monthly_limit {
            if monthly_usage + requested_tokens > monthly_limit {
                return Ok(false);
            }
        }

        Ok(true)
    }

    pub async fn get_remaining(
        &self,
        provider_id: &str,
        quota: &ProviderQuota,
    ) -> QuotaRemaining {
        let daily_usage = self.get_daily_usage(provider_id).await;
        let monthly_usage = self.get_monthly_usage(provider_id).await;

        QuotaRemaining {
            daily_remaining: quota.daily_limit.map(|limit| limit - daily_usage),
            monthly_remaining: quota.monthly_limit.map(|limit| limit - monthly_usage),
            daily_percentage: quota
                .daily_limit
                .map(|limit| (daily_usage as f64 / limit as f64 * 100.0).round() / 100.0),
            monthly_percentage: quota
                .monthly_limit
                .map(|limit| (monthly_usage as f64 / limit as f64 * 100.0).round() / 100.0),
        }
    }

    pub async fn reset_daily(&self) {
        // In production, call this once per day via background job
        self.daily_usage.clear();
        tracing::info!("Daily quotas reset");
    }

    pub async fn reset_monthly(&self) {
        // Call this once per month
        self.monthly_usage.clear();
        tracing::info!("Monthly quotas reset");
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct QuotaRemaining {
    pub daily_remaining: Option<i64>,
    pub monthly_remaining: Option<i64>,
    pub daily_percentage: Option<f64>,
    pub monthly_percentage: Option<f64>,
}
```

## Quota Validation

```rust
pub async fn validate_request(
    tracker: &QuotaTracker,
    provider: &Provider,
    estimated_tokens: i64,
) -> Result<(), QuotaError> {
    let quota = ProviderQuota {
        provider_id: provider.id.clone(),
        daily_limit: provider.free_quota_daily,
        monthly_limit: provider.free_quota_monthly,
        weekly_limit: None,
        requests_per_minute: provider.rate_limit_rpm,
    };

    if !tracker.check_quota(&provider.id, &quota, estimated_tokens).await? {
        let remaining = tracker.get_remaining(&provider.id, &quota).await;
        return Err(QuotaError::DailyLimitExceeded {
            provider_id: provider.id.clone(),
            remaining: remaining.daily_remaining,
            used: tracker.get_daily_usage(&provider.id).await,
        });
    }

    Ok(())
}
```

## Rate Limiting (Per-Minute)

```rust
pub struct RateLimiter {
    // Key: "provider_id:minute" (e.g., "groq:2026-09-27T14:01")
    minute_usage: Arc<DashMap<String, i32>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        RateLimiter {
            minute_usage: Arc::new(DashMap::new()),
        }
    }

    fn get_minute_key(provider_id: &str) -> String {
        let minute = chrono::Utc::now().format("%Y-%m-%dT%H:%M").to_string();
        format!("{}:{}", provider_id, minute)
    }

    pub async fn check_rate_limit(
        &self,
        provider_id: &str,
        limit_rpm: i32,
    ) -> Result<bool, RateLimitError> {
        let key = Self::get_minute_key(provider_id);
        let current_count = self.minute_usage.get(&key).map(|v| *v).unwrap_or(0);

        if current_count >= limit_rpm {
            return Ok(false);
        }

        self.minute_usage
            .entry(key)
            .and_modify(|v| *v += 1)
            .or_insert(1);

        Ok(true)
    }

    pub async fn get_requests_this_minute(&self, provider_id: &str) -> i32 {
        let key = Self::get_minute_key(provider_id);
        self.minute_usage.get(&key).map(|v| *v).unwrap_or(0)
    }

    pub async fn reset_minute_counters(&self) {
        // Call this every minute via background job
        self.minute_usage.clear();
    }
}
```

## Alert Thresholds

```rust
pub struct QuotaAlertConfig {
    pub warning_threshold: f64,     // Alert at 70%
    pub critical_threshold: f64,    // Alert at 90%
}

pub async fn check_quota_alerts(
    tracker: &QuotaTracker,
    provider: &Provider,
    config: &QuotaAlertConfig,
) -> Vec<QuotaAlert> {
    let mut alerts = Vec::new();

    let quota = ProviderQuota {
        provider_id: provider.id.clone(),
        daily_limit: provider.free_quota_daily,
        monthly_limit: provider.free_quota_monthly,
        weekly_limit: None,
        requests_per_minute: provider.rate_limit_rpm,
    };

    let remaining = tracker.get_remaining(&provider.id, &quota).await;

    if let Some(daily_pct) = remaining.daily_percentage {
        if daily_pct >= config.critical_threshold {
            alerts.push(QuotaAlert::DailyCritical {
                provider_id: provider.id.clone(),
                percentage_used: daily_pct,
                remaining_tokens: remaining.daily_remaining,
            });
        } else if daily_pct >= config.warning_threshold {
            alerts.push(QuotaAlert::DailyWarning {
                provider_id: provider.id.clone(),
                percentage_used: daily_pct,
            });
        }
    }

    if let Some(monthly_pct) = remaining.monthly_percentage {
        if monthly_pct >= config.critical_threshold {
            alerts.push(QuotaAlert::MonthlyCritical {
                provider_id: provider.id.clone(),
                percentage_used: monthly_pct,
            });
        }
    }

    alerts
}

#[derive(Debug, Clone, Serialize)]
pub enum QuotaAlert {
    DailyWarning { provider_id: String, percentage_used: f64 },
    DailyCritical { provider_id: String, percentage_used: f64, remaining_tokens: Option<i64> },
    MonthlyCritical { provider_id: String, percentage_used: f64 },
}
```

## Background Jobs for Reset

```rust
pub async fn start_quota_background_jobs(tracker: Arc<QuotaTracker>) {
    // Daily reset at midnight
    tokio::spawn(async move {
        loop {
            let now = chrono::Local::now();
            let next_midnight = (now + chrono::Duration::days(1))
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .unwrap();
            let duration = (next_midnight - now.naive_local()).to_std().unwrap();

            tokio::time::sleep(duration).await;
            tracker.reset_daily().await;
        }
    });

    // Monthly reset on first day of month
    tokio::spawn(async move {
        loop {
            let now = chrono::Local::now();
            let next_month = if now.month() == 12 {
                now.with_year(now.year() + 1)
                    .unwrap()
                    .with_month(1)
                    .unwrap()
            } else {
                now.with_month(now.month() + 1).unwrap()
            };
            let next_reset = next_month
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .unwrap();
            let duration = (next_reset - now.naive_local()).to_std().unwrap();

            tokio::time::sleep(duration).await;
            tracker.reset_monthly().await;
        }
    });
}
```

## Usage in Handler

```rust
pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Json<ChatCompletionResponse>, ApiError> {
    let provider = select_provider(&state.db_pool, &req.model).await?;
    let estimated_tokens = estimate_tokens(&req.messages);

    // Validate quota
    validate_request(&state.quota_tracker, &provider, estimated_tokens).await?;

    // Check rate limit
    state.rate_limiter
        .check_rate_limit(&provider.id, provider.rate_limit_rpm)
        .await?;

    // Call provider...

    // Record usage
    state.quota_tracker.add_tokens(&provider.id, actual_tokens).await;

    Ok(Json(response))
}
```

## Configuration

```toml
[quota]
warning_threshold = 0.70
critical_threshold = 0.90

[rate_limit]
enable_rate_limiting = true

[reset_jobs]
enable_daily_reset = true
enable_monthly_reset = true
```
