use sqlx::PgPool;

use crate::error::AppResult;
use crate::security::tokens::hash_token;

const STATE_TTL_SECS: f64 = 600.0;

pub async fn create(pool: &PgPool, state: &str, verifier: &str, ip: &str) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO oauth_states (state_hash, code_verifier, ip, expires_at) \
         VALUES ($1, $2, $3, now() + make_interval(secs => $4::double precision))",
    )
    .bind(hash_token(state))
    .bind(verifier)
    .bind(ip)
    .bind(STATE_TTL_SECS)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn take(pool: &PgPool, state: &str) -> AppResult<Option<String>> {
    let verifier: Option<String> = sqlx::query_scalar(
        "DELETE FROM oauth_states WHERE state_hash = $1 AND expires_at > now() RETURNING code_verifier",
    )
    .bind(hash_token(state))
    .fetch_optional(pool)
    .await?;
    Ok(verifier)
}
