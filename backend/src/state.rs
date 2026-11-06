// backend/src/state.rs
use std::ops::Deref;
use std::sync::Arc;

use axum::http::HeaderValue;
use sqlx::PgPool;

use crate::config::Config;
use crate::database::site::SiteCache;
use crate::security::ratelimit::RateLimiter;

pub struct Inner {
    pub cfg: Config,
    pub pool: PgPool,
    pub http: reqwest::Client,
    pub limiter: RateLimiter,
    pub cache: SiteCache,
    pub index_template: String,
    pub csp_public: HeaderValue,
    pub csp_admin: HeaderValue,
}

#[derive(Clone)]
pub struct AppState(pub Arc<Inner>);

impl Deref for AppState {
    type Target = Inner;

    fn deref(&self) -> &Inner {
        &self.0
    }
}
