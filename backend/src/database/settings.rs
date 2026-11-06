use std::collections::HashMap;

use serde::de::DeserializeOwned;
use serde::Serialize;
use sqlx::types::Json;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{GeneralSettings, SeoSettings};

async fn get_json<T: DeserializeOwned + Send + Unpin + 'static>(
    pool: &PgPool,
    key: &str,
) -> AppResult<T> {
    let row: Option<Json<T>> = sqlx::query_scalar("SELECT value FROM site_settings WHERE key = $1")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    match row {
        Some(Json(v)) => Ok(v),
        None => Err(AppError::Internal(format!("impostazione mancante: {key}"))),
    }
}

pub async fn get_general(pool: &PgPool) -> AppResult<GeneralSettings> {
    get_json(pool, "general").await
}

pub async fn get_seo(pool: &PgPool) -> AppResult<SeoSettings> {
    get_json(pool, "seo").await
}

pub async fn put<T: Serialize + Sync>(
    conn: &mut PgConnection,
    key: &str,
    value: &T,
    actor: Option<Uuid>,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO site_settings (key, value, updated_by) VALUES ($1, $2, $3) \
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = now(), updated_by = EXCLUDED.updated_by",
    )
    .bind(key)
    .bind(Json(value))
    .bind(actor)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn ui_texts(pool: &PgPool, lang: &str) -> AppResult<Vec<(String, String)>> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT key, value FROM ui_texts WHERE lang = $1 ORDER BY key")
            .bind(lang)
            .fetch_all(pool)
            .await?;
    Ok(rows)
}

pub async fn ui_texts_all(pool: &PgPool) -> AppResult<HashMap<String, Vec<(String, String)>>> {
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT lang, key, value FROM ui_texts ORDER BY lang, key")
            .fetch_all(pool)
            .await?;
    let mut out: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for (lang, key, value) in rows {
        out.entry(lang).or_default().push((key, value));
    }
    Ok(out)
}

pub async fn upsert_ui_texts(
    conn: &mut PgConnection,
    lang: &str,
    items: &[(String, String)],
) -> AppResult<()> {
    for (key, value) in items {
        sqlx::query(
            "INSERT INTO ui_texts (lang, key, value) VALUES ($1, $2, $3) \
             ON CONFLICT (lang, key) DO UPDATE SET value = EXCLUDED.value, updated_at = now()",
        )
        .bind(lang)
        .bind(key)
        .bind(value)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}
