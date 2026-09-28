use crate::providers::Provider;
use crate::quota::DbPool;
use std::collections::HashMap;

#[derive(Clone)]
pub struct AppState {
    pub db_pool: DbPool,
    pub providers: HashMap<String, Provider>,
}
