use rust_agent::{
    providers::{
        circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, CircuitState},
        types::{Provider, ProviderHealth},
        ProviderRegistry,
    },
    quota::tracker::create_memory_pool,
    routing::{fallback::FallbackSelector, selector::Router, strategies::RoutingStrategy},
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn test_circuit_breaker_state_transitions() {
    let cb = Arc::new(CircuitBreaker::with_config(
        "test_failing_provider".to_string(),
        CircuitBreakerConfig {
            failure_threshold: 3,
            recovery_timeout: Duration::from_millis(80),
            success_threshold: 2,
        },
    ));

    assert_eq!(cb.state().await, CircuitState::Closed);

    // Trip circuit breaker with 3 consecutive failures
    cb.record_failure().await;
    cb.record_failure().await;
    cb.record_failure().await;
    assert_eq!(cb.state().await, CircuitState::Open);

    // Wait for recovery timeout to transition to HalfOpen
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(cb.state().await, CircuitState::HalfOpen);

    // 2 successes in HalfOpen will close the circuit
    cb.record_success().await;
    assert_eq!(cb.state().await, CircuitState::HalfOpen);
    cb.record_success().await;
    assert_eq!(cb.state().await, CircuitState::Closed);
}

#[tokio::test]
async fn test_fallback_selector_multi_tier_resilience() {
    let pool = create_memory_pool().unwrap();

    let mut p1 = Provider::new(
        "p1".to_string(),
        "P1".to_string(),
        "https://p1.com".to_string(),
        "api_key".to_string(),
        Some(100),
        None,
        10,
        vec!["model-a".to_string()],
    );
    p1.health_status = ProviderHealth::Down; // p1 is down

    let mut p2 = Provider::new(
        "p2".to_string(),
        "P2".to_string(),
        "https://p2.com".to_string(),
        "api_key".to_string(),
        Some(100),
        None,
        10,
        vec!["model-a".to_string()],
    );
    p2.health_status = ProviderHealth::Healthy;

    let mut p3 = Provider::new(
        "p3".to_string(),
        "P3".to_string(),
        "https://p3.com".to_string(),
        "none".to_string(),
        Some(100),
        None,
        10,
        vec!["model-b".to_string()],
    );
    p3.health_status = ProviderHealth::Healthy;

    let mut providers = HashMap::new();
    providers.insert("p1".to_string(), p1);
    providers.insert("p2".to_string(), p2);
    providers.insert("p3".to_string(), p3);

    let fallback_selector = FallbackSelector::new(providers.clone());

    // Primary p1 is down, but p2 supports model-a and is healthy -> chooses p2
    let chosen = fallback_selector
        .select_with_fallback(&pool, Some("p1"), Some("model-a"))
        .await
        .expect("Fallback to p2");
    assert_eq!(chosen.id, "p2");

    // Fallback for non-existent model returns healthy provider p2 or p3
    let any_fallback = fallback_selector
        .select_with_fallback(&pool, Some("p1"), Some("unknown-model"))
        .await
        .expect("Fallback to any healthy provider");
    assert!(any_fallback.id == "p2" || any_fallback.id == "p3");
}

#[tokio::test]
async fn test_router_priority_and_quota_selection() {
    let pool = create_memory_pool().unwrap();

    let p1 = Provider::new(
        "p_groq".to_string(),
        "Groq".to_string(),
        "https://api.groq.com".to_string(),
        "api_key".to_string(),
        Some(500),
        None,
        30,
        vec!["llama3".to_string()],
    );

    let p2 = Provider::new(
        "p_together".to_string(),
        "Together".to_string(),
        "https://api.together.xyz".to_string(),
        "api_key".to_string(),
        Some(5000),
        None,
        60,
        vec!["llama3".to_string()],
    );

    let mut providers = HashMap::new();
    providers.insert("p_groq".to_string(), p1);
    providers.insert("p_together".to_string(), p2);

    let registry = ProviderRegistry::new(providers);
    let router = Router::new(registry, RoutingStrategy::QuotaFirst);

    let selected = router
        .select_provider(&pool, Some("llama3"))
        .await
        .expect("QuotaFirst provider selection");

    // QuotaFirst selects provider with higher remaining quota (p_together has 5000 vs 500)
    assert_eq!(selected.id, "p_together");
}
