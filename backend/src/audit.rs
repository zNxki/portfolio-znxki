use serde_json::{json, Value};
use sqlx::types::Json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::AppResult;
use crate::models::AuditEntry;

pub struct Event {
    pub severity: &'static str,
    pub actor_id: Option<Uuid>,
    pub actor_login: Option<String>,
    pub action: String,
    pub entity: Option<String>,
    pub entity_id: Option<String>,
    pub ip: Option<String>,
    pub details: Value,
}

impl Event {
    pub fn new(action: &str) -> Self {
        Event {
            severity: "info",
            actor_id: None,
            actor_login: None,
            action: action.to_string(),
            entity: None,
            entity_id: None,
            ip: None,
            details: json!({}),
        }
    }

    pub fn security(mut self) -> Self {
        self.severity = "security";
        self
    }

    pub fn warning(mut self) -> Self {
        self.severity = "warning";
        self
    }

    pub fn actor(mut self, user: &CurrentUser) -> Self {
        self.actor_id = Some(user.id);
        self.actor_login = Some(user.login.clone());
        self
    }

    pub fn actor_raw(mut self, id: Uuid, login: &str) -> Self {
        self.actor_id = Some(id);
        self.actor_login = Some(login.to_string());
        self
    }

    pub fn entity(mut self, name: &str, id: impl ToString) -> Self {
        self.entity = Some(name.to_string());
        self.entity_id = Some(id.to_string());
        self
    }

    pub fn ip(mut self, ip: &str) -> Self {
        self.ip = Some(ip.to_string());
        self
    }

    pub fn details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }
}

pub async fn record<'e, E>(exec: E, ev: Event) -> Result<(), sqlx::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    tracing::info!(
        target: "audit",
        action = %ev.action,
        severity = ev.severity,
        actor = ev.actor_login.as_deref().unwrap_or("-"),
        entity = ev.entity.as_deref().unwrap_or("-"),
        entity_id = ev.entity_id.as_deref().unwrap_or("-"),
        ip = ev.ip.as_deref().unwrap_or("-"),
        "audit"
    );
    sqlx::query(
        "INSERT INTO audit_log (severity, actor_id, actor_login, action, entity, entity_id, ip, details) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(ev.severity)
    .bind(ev.actor_id)
    .bind(ev.actor_login)
    .bind(ev.action)
    .bind(ev.entity)
    .bind(ev.entity_id)
    .bind(ev.ip)
    .bind(Json(ev.details))
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn record_lossy(pool: &PgPool, ev: Event) {
    if let Err(e) = record(pool, ev).await {
        tracing::error!(error = %e, "scrittura audit log fallita");
    }
}

pub struct AuditFilter {
    pub action: Option<String>,
    pub severity: Option<String>,
    pub limit: i64,
    pub offset: i64,
}

pub async fn list(pool: &PgPool, f: &AuditFilter) -> AppResult<(Vec<AuditEntry>, i64)> {
    let action_like = f.action.as_ref().map(|a| format!("{a}%"));
    let rows = sqlx::query_as::<_, AuditEntry>(
        "SELECT id, at, severity, actor_id, actor_login, action, entity, entity_id, ip, details \
         FROM audit_log \
         WHERE ($1::text IS NULL OR action LIKE $1) AND ($2::text IS NULL OR severity = $2) \
         ORDER BY at DESC, id DESC LIMIT $3 OFFSET $4",
    )
    .bind(&action_like)
    .bind(&f.severity)
    .bind(f.limit)
    .bind(f.offset)
    .fetch_all(pool)
    .await?;
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log WHERE ($1::text IS NULL OR action LIKE $1) AND ($2::text IS NULL OR severity = $2)",
    )
    .bind(&action_like)
    .bind(&f.severity)
    .fetch_one(pool)
    .await?;
    Ok((rows, total))
}