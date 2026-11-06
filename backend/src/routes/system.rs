// backend/src/routes/system.rs
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use super::ev;
use crate::audit::{self, AuditFilter};
use crate::auth::extractors::{AdminUser, ClientIp};
use crate::auth::session;
use crate::database::{projects, settings, stats, users};
use crate::error::{AppError, AppResult};
use crate::models::{AuditEntry, SessionView, User};
use crate::security::validation::Errors;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/dashboard", get(dashboard))
        .route("/api/admin/users", get(list_users))
        .route(
            "/api/admin/users/{id}/revoke-sessions",
            post(revoke_user_sessions),
        )
        .route("/api/admin/sessions", get(list_sessions))
        .route("/api/admin/sessions/{id}", delete(revoke_session))
        .route("/api/admin/audit", get(audit_log))
}

async fn dashboard(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AdminUser(user): AdminUser,
) -> AppResult<Json<Value>> {
    let counters = stats::collect(&st.pool).await?;
    let general = settings::get_general(&st.pool).await?;
    let filter = AuditFilter {
        action: Some("content.".to_string()),
        severity: None,
        limit: 10,
        offset: 0,
    };
    let (recent_changes, _) = audit::list(&st.pool, &filter).await?;

    let mut all = projects::list_plain(&st.pool).await?;
    all.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    let recent_projects: Vec<Value> = all
        .iter()
        .take(5)
        .map(|p| {
            json!({
                "id": p.id,
                "slug": p.slug,
                "title": p.title.0.pick(&general.default_lang, &general.default_lang),
                "visible": p.visible,
                "status": p.status,
                "updated_at": p.updated_at
            })
        })
        .collect();

    audit::record_lossy(&st.pool, ev("admin.dashboard_view", &user, &ip)).await;

    Ok(Json(json!({
        "stats": counters,
        "recent_changes": recent_changes,
        "recent_projects": recent_projects
    })))
}

async fn list_users(
    State(st): State<AppState>,
    AdminUser(_): AdminUser,
) -> AppResult<Json<Vec<User>>> {
    Ok(Json(users::list(&st.pool).await?))
}

async fn revoke_user_sessions(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AdminUser(user): AdminUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    users::get(&st.pool, id).await?;
    let except = if id == user.id {
        Some(user.session_id)
    } else {
        None
    };
    let revoked = session::revoke_all_for_user(&st.pool, id, except).await?;
    audit::record_lossy(
        &st.pool,
        ev("admin.sessions_revoked", &user, &ip)
            .security()
            .entity("user", id)
            .details(json!({ "count": revoked })),
    )
    .await;
    Ok(Json(json!({ "revoked": revoked })))
}

async fn list_sessions(
    State(st): State<AppState>,
    AdminUser(_): AdminUser,
) -> AppResult<Json<Vec<SessionView>>> {
    Ok(Json(session::list_active(&st.pool, None).await?))
}

async fn revoke_session(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AdminUser(user): AdminUser,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    if !session::revoke(&st.pool, id).await? {
        return Err(AppError::NotFound);
    }
    audit::record_lossy(
        &st.pool,
        ev("admin.session_revoked", &user, &ip)
            .security()
            .entity("session", id),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct AuditQuery {
    action: Option<String>,
    severity: Option<String>,
    page: Option<i64>,
    per_page: Option<i64>,
}

fn build_audit_filter(q: &AuditQuery) -> AppResult<(AuditFilter, i64, i64)> {
    let mut e = Errors::new();
    let action = match q.action.as_deref().map(str::trim) {
        Some(a) if !a.is_empty() => {
            let ok = a.len() <= 60
                && a.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_');
            if !ok {
                e.push("action", "Filtro azione non valido");
            }
            Some(a.to_string())
        }
        _ => None,
    };
    let severity = match q.severity.as_deref().map(str::trim) {
        Some(s) if !s.is_empty() => {
            if !matches!(s, "info" | "warning" | "security") {
                e.push("severity", "Severità non valida");
            }
            Some(s.to_string())
        }
        _ => None,
    };
    let per_page = q.per_page.unwrap_or(50);
    let page = q.page.unwrap_or(0);
    if !(1..=100).contains(&per_page) {
        e.push("per_page", "Valore fuori intervallo (1-100)");
    }
    if !(0..=10_000).contains(&page) {
        e.push("page", "Valore fuori intervallo");
    }
    e.finish()?;
    Ok((
        AuditFilter {
            action,
            severity,
            limit: per_page,
            offset: page * per_page,
        },
        page,
        per_page,
    ))
}

async fn audit_log(
    State(st): State<AppState>,
    AdminUser(_): AdminUser,
    Query(q): Query<AuditQuery>,
) -> AppResult<Json<Value>> {
    let (filter, page, per_page) = build_audit_filter(&q)?;
    let (items, total): (Vec<AuditEntry>, i64) = audit::list(&st.pool, &filter).await?;
    Ok(Json(json!({
        "items": items,
        "total": total,
        "page": page,
        "per_page": per_page
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(
        action: Option<&str>,
        severity: Option<&str>,
        page: Option<i64>,
        per: Option<i64>,
    ) -> AuditQuery {
        AuditQuery {
            action: action.map(str::to_string),
            severity: severity.map(str::to_string),
            page,
            per_page: per,
        }
    }

    #[test]
    fn audit_filter_defaults() {
        let r = build_audit_filter(&q(None, None, None, None));
        assert!(r.is_ok());
        if let Ok((f, page, per)) = r {
            assert_eq!(f.limit, 50);
            assert_eq!(f.offset, 0);
            assert_eq!((page, per), (0, 50));
        }
    }

    #[test]
    fn audit_filter_pagination() {
        let r = build_audit_filter(&q(Some("content."), Some("security"), Some(3), Some(20)));
        assert!(r.is_ok());
        if let Ok((f, _, _)) = r {
            assert_eq!(f.offset, 60);
            assert_eq!(f.action.as_deref(), Some("content."));
        }
    }

    #[test]
    fn audit_filter_rejects_bad_input() {
        assert!(build_audit_filter(&q(Some("a%b"), None, None, None)).is_err());
        assert!(build_audit_filter(&q(Some("x'; drop"), None, None, None)).is_err());
        assert!(build_audit_filter(&q(None, Some("fatal"), None, None)).is_err());
        assert!(build_audit_filter(&q(None, None, Some(-1), None)).is_err());
        assert!(build_audit_filter(&q(None, None, None, Some(1000))).is_err());
    }
}
