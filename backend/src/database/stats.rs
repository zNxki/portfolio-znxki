use serde::Serialize;
use sqlx::{FromRow, PgPool};

use crate::error::AppResult;

#[derive(Debug, Serialize, FromRow)]
pub struct Stats {
    pub projects_total: i64,
    pub projects_visible: i64,
    pub experiences: i64,
    pub technologies: i64,
    pub contacts: i64,
    pub media: i64,
    pub users: i64,
    pub admins: i64,
    pub active_sessions: i64,
}

pub async fn collect(pool: &PgPool) -> AppResult<Stats> {
    let row = sqlx::query_as::<_, Stats>(
        "SELECT \
            (SELECT count(*) FROM projects) AS projects_total, \
            (SELECT count(*) FROM projects WHERE visible) AS projects_visible, \
            (SELECT count(*) FROM experiences) AS experiences, \
            (SELECT count(*) FROM technologies) AS technologies, \
            (SELECT count(*) FROM contacts) AS contacts, \
            (SELECT count(*) FROM media) AS media, \
            (SELECT count(*) FROM users) AS users, \
            (SELECT count(*) FROM users WHERE role = 'ADMIN') AS admins, \
            (SELECT count(*) FROM sessions WHERE revoked_at IS NULL AND expires_at > now()) AS active_sessions",
    )
    .fetch_one(pool)
    .await?;
    Ok(row)
}
