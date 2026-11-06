use std::convert::Infallible;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use super::CurrentUser;
use crate::audit::{self, Event};
use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Clone)]
pub struct ClientIp(pub String);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &AppState) -> Result<Self, Self::Rejection> {
        Ok(parts
            .extensions
            .get::<ClientIp>()
            .cloned()
            .unwrap_or_else(|| ClientIp("unknown".to_string())))
    }
}

#[derive(Debug, Clone)]
pub struct AuthUser(pub CurrentUser);

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &AppState) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<CurrentUser>()
            .cloned()
            .map(AuthUser)
            .ok_or(AppError::Unauthorized)
    }
}

#[derive(Debug, Clone)]
pub struct AdminUser(pub CurrentUser);

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let user = parts
            .extensions
            .get::<CurrentUser>()
            .cloned()
            .ok_or(AppError::Unauthorized)?;
        if !user.is_admin() {
            let ip = parts
                .extensions
                .get::<ClientIp>()
                .map(|c| c.0.clone())
                .unwrap_or_default();
            audit::record_lossy(
                &state.pool,
                Event::new("security.admin_denied")
                    .security()
                    .actor(&user)
                    .ip(&ip)
                    .details(serde_json::json!({ "path": parts.uri.path() })),
            )
            .await;
            return Err(AppError::Forbidden);
        }
        Ok(AdminUser(user))
    }
}