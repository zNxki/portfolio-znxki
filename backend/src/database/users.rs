// backend/src/database/users.rs
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::auth::github::GithubUser;
use crate::error::AppResult;
use crate::models::{Role, User};

const COLS: &str =
    "id, github_id, login, display_name, avatar_url, role, created_at, last_login_at";

/// Autorizza l'accesso solo agli ID GitHub presenti in `ADMIN_GITHUB_IDS`.
/// Nessun altro account può ottenere una sessione, nemmeno se il database è vuoto.
pub async fn login_allowed(
    pool: &PgPool,
    gh: &GithubUser,
    allowed: &[i64],
) -> AppResult<Option<User>> {
    if !allowed.contains(&gh.id) {
        return Ok(None);
    }
    let row = sqlx::query_as::<_, User>(&format!(
        "INSERT INTO users (github_id, login, display_name, avatar_url, role) \
         VALUES ($1, $2, $3, $4, 'ADMIN') \
         ON CONFLICT (github_id) DO UPDATE SET login = EXCLUDED.login, \
            display_name = EXCLUDED.display_name, avatar_url = EXCLUDED.avatar_url, \
            last_login_at = now(), role = 'ADMIN' \
         RETURNING {COLS}"
    ))
    .bind(gh.id)
    .bind(&gh.login)
    .bind(&gh.name)
    .bind(&gh.avatar_url)
    .fetch_one(pool)
    .await?;
    Ok(Some(row))
}

/// Allinea i ruoli all'allowlist: chi non è più elencato in `ADMIN_GITHUB_IDS`
/// viene declassato a USER e le sue sessioni vengono revocate.
pub async fn sync_admins(pool: &PgPool, allowed: &[i64]) -> AppResult<u64> {
    let mut tx = pool.begin().await?;
    let demoted: Vec<Uuid> = sqlx::query_scalar(
        "UPDATE users SET role = 'USER' WHERE role = 'ADMIN' AND github_id <> ALL($1::bigint[]) RETURNING id",
    )
    .bind(allowed)
    .fetch_all(&mut *tx)
    .await?;
    if !demoted.is_empty() {
        sqlx::query("UPDATE sessions SET revoked_at = now() WHERE user_id = ANY($1) AND revoked_at IS NULL")
            .bind(&demoted)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(demoted.len() as u64)
}

pub async fn list(pool: &PgPool) -> AppResult<Vec<User>> {
    let rows = sqlx::query_as::<_, User>(&format!("SELECT {COLS} FROM users ORDER BY created_at"))
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn get(pool: &PgPool, id: Uuid) -> AppResult<User> {
    let row = sqlx::query_as::<_, User>(&format!("SELECT {COLS} FROM users WHERE id = $1"))
        .bind(id)
        .fetch_one(pool)
        .await?;
    Ok(row)
}

pub async fn lock_admins(conn: &mut PgConnection) -> AppResult<i64> {
    let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE role = 'ADMIN' FOR UPDATE")
        .fetch_all(&mut *conn)
        .await?;
    Ok(ids.len() as i64)
}

pub async fn set_role(conn: &mut PgConnection, id: Uuid, role: Role) -> AppResult<(User, String)> {
    let previous: String = sqlx::query_scalar("SELECT role FROM users WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
    let row = sqlx::query_as::<_, User>(&format!(
        "UPDATE users SET role = $2 WHERE id = $1 RETURNING {COLS}"
    ))
    .bind(id)
    .bind(role.as_str())
    .fetch_one(&mut *conn)
    .await?;
    Ok((row, previous))
}
