pub mod fallback;
pub mod selector;
pub mod strategies;

pub use fallback::FallbackSelector;
pub use selector::Router;
pub use strategies::RoutingStrategy;
