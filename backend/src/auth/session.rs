use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use super::CurrentUser;
use crate::config::Config;
use crate::error::{AppError, AppResult};
use crate::models::{Role, SessionView};
use crate::security::tokens::{csrf_token, hash_token, random_token};

#[derive(Debug, FromRow)]
struct ResolveRow {
    session_id: Uuid,
    via_current: bool,
    needs_rotation: bool,
    needs_touch: bool,
    remaining_secs: i64,
    user_id: Uuid,
    github_id: i64,
    login: String,
    display_name: Option<String>,
    avatar_url: Option<String>,
    role: String,
}

pub struct Rotated {
    pub token: String,
    pub max_age: i64,
}

pub struct Resolved {
    pub user: CurrentUser,
    pub rotated: Option<Rotated>,
}

const MAX_SESSIONS_PER_USER: i64 = 20;

pub async fn create(
    pool: &PgPool,
    cfg: &Config,
    user_id: Uuid,
    ip: &str,
    user_agent: Option<&str>,
) -> AppResult<Rotated> {
    let token = random_token();
    let ua: Option<String> = user_agent.map(|u| u.chars().take(300).collect());
    sqlx::query(
        "INSERT INTO sessions (user_id, token_hash, expires_at, ip, user_agent) \
         VALUES ($1, $2, now() + make_interval(secs => $3::double precision), $4, $5)",
    )
    .bind(user_id)
    .bind(hash_token(&token))
    .bind(cfg.session_absolute_secs as f64)
    .bind(ip)
    .bind(ua)
    .execute(pool)
    .await?;

    sqlx::query(
        "DELETE FROM sessions WHERE user_id = $1 AND id NOT IN \
         (SELECT id FROM sessions WHERE user_id = $1 ORDER BY created_at DESC LIMIT $2)",
    )
    .bind(user_id)
    .bind(MAX_SESSIONS_PER_USER)
    .execute(pool)
    .await?;

    Ok(Rotated {
        token,
        max_age: cfg.session_absolute_secs,
    })
}

pub async fn resolve(pool: &PgPool, cfg: &Config, token: &str) -> AppResult<Option<Resolved>> {
    let hash = hash_token(token);
    let row = sqlx::query_as::<_, ResolveRow>(
        "SELECT s.id AS session_id, \
                (s.token_hash = $1) AS via_current, \
                ((now() - s.rotated_at) > make_interval(secs => $3::double precision)) AS needs_rotation, \
                ((now() - s.last_seen_at) > interval '60 seconds') AS needs_touch, \
                EXTRACT(EPOCH FROM (s.expires_at - now()))::bigint AS remaining_secs, \
                u.id AS user_id, u.github_id, u.login, u.display_name, u.avatar_url, u.role \
         FROM sessions s JOIN users u ON u.id = s.user_id \
         WHERE (s.token_hash = $1 OR (s.prev_token_hash = $1 AND s.rotated_at > now() - interval '30 seconds')) \
           AND s.revoked_at IS NULL \
           AND s.expires_at > now() \
           AND s.last_seen_at > now() - make_interval(secs => $2::double precision) \
         LIMIT 1",
    )
    .bind(&hash)
    .bind(cfg.session_idle_secs as f64)
    .bind(cfg.session_rotate_secs as f64)
    .fetch_optional(pool)
    .await?;

    let row = match row {
        Some(r) => r,
        None => return Ok(None),
    };

    let mut rotated = None;
    if row.via_current && row.needs_rotation {
        let new_token = random_token();
        let res = sqlx::query(
            "UPDATE sessions SET prev_token_hash = token_hash, token_hash = $2, \
             rotated_at = now(), last_seen_at = now() WHERE id = $1 AND token_hash = $3",
        )
        .bind(row.session_id)
        .bind(hash_token(&new_token))
        .bind(&hash)
        .execute(pool)
        .await?;
        if res.rows_affected() == 1 {
            rotated = Some(Rotated {
                token: new_token,
                max_age: row.remaining_secs.max(1),
            });
        }
    } else if row.needs_touch {
        sqlx::query("UPDATE sessions SET last_seen_at = now() WHERE id = $1")
            .bind(row.session_id)
            .execute(pool)
            .await?;
    }

    let csrf = csrf_token(&cfg.app_secret, &row.session_id)?;
    Ok(Some(Resolved {
        user: CurrentUser {
            id: row.user_id,
            github_id: row.github_id,
            login: row.login,
            display_name: row.display_name,
            avatar_url: row.avatar_url,
            role: Role::parse(&row.role).unwrap_or(Role::User),
            session_id: row.session_id,
            csrf,
        },
        rotated,
    }))
}

pub async fn revoke_by_token(pool: &PgPool, token: &str) -> AppResult<()> {
    sqlx::query("UPDATE sessions SET revoked_at = now() WHERE token_hash = $1 AND revoked_at IS NULL")
        .bind(hash_token(token))
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn revoke(pool: &PgPool, session_id: Uuid) -> AppResult<bool> {
    let res = sqlx::query("UPDATE sessions SET revoked_at = now() WHERE id = $1 AND revoked_at IS NULL")
        .bind(session_id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn revoke_owned(pool: &PgPool, session_id: Uuid, user_id: Uuid) -> AppResult<bool> {
    let res = sqlx::query(
        "UPDATE sessions SET revoked_at = now() WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL",
    )
    .bind(session_id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn revoke_all_for_user(pool: &PgPool, user_id: Uuid, except: Option<Uuid>) -> AppResult<u64> {
    let res = sqlx::query(
        "UPDATE sessions SET revoked_at = now() \
         WHERE user_id = $1 AND revoked_at IS NULL AND ($2::uuid IS NULL OR id <> $2)",
    )
    .bind(user_id)
    .bind(except)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

pub async fn list_active(pool: &PgPool, user_id: Option<Uuid>) -> AppResult<Vec<SessionView>> {
    let rows = sqlx::query_as::<_, SessionView>(
        "SELECT id, user_id, created_at, last_seen_at, expires_at, ip, user_agent FROM sessions \
         WHERE revoked_at IS NULL AND expires_at > now() AND ($1::uuid IS NULL OR user_id = $1) \
         ORDER BY last_seen_at DESC LIMIT 200",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn purge_expired(pool: &PgPool) -> Result<(), AppError> {
    sqlx::query(
        "DELETE FROM sessions WHERE expires_at < now() - interval '7 days' \
         OR (revoked_at IS NOT NULL AND revoked_at < now() - interval '7 days')",
    )
    .execute(pool)
    .await?;
    sqlx::query("DELETE FROM oauth_states WHERE expires_at < now()")
        .execute(pool)
        .await?;
    Ok(())
}