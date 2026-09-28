#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub enum RoutingStrategy {
    #[default]
    Auto,
    Priority,
    RoundRobin,
    QuotaFirst,
    LatencyFirst,
}
