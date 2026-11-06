// backend/src/routes/mod.rs
pub mod auth;
pub mod content;
pub mod extract;
pub mod media;
pub mod public;
pub mod settings;
pub mod system;

use axum::extract::DefaultBodyLimit;
use axum::Router;
use sqlx::{Postgres, Transaction};

use crate::audit::Event;
use crate::auth::CurrentUser;
use crate::config::Config;
use crate::error::AppResult;
use crate::state::AppState;

pub fn ev(action: &str, user: &CurrentUser, ip: &str) -> Event {
    Event::new(action).actor(user).ip(ip)
}

pub async fn commit(st: &AppState, tx: Transaction<'_, Postgres>) -> AppResult<()> {
    tx.commit().await?;
    st.cache.invalidate().await;
    Ok(())
}

pub fn routes(cfg: &Config) -> Router<AppState> {
    let json_routes = Router::new()
        .merge(auth::routes())
        .merge(content::routes())
        .merge(settings::routes())
        .merge(system::routes())
        .layer(DefaultBodyLimit::max(cfg.max_json_bytes));
    let upload_routes = media::routes().layer(DefaultBodyLimit::max(cfg.max_upload_bytes + 65_536));
    Router::new()
        .merge(public::routes())
        .merge(json_routes)
        .merge(upload_routes)
        .fallback(public::static_file)
}
