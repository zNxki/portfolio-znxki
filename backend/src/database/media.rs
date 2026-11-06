use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::Media;

const COLS: &str = "id, filename, original_name, mime, size_bytes, sha256, created_at";

pub async fn list(pool: &PgPool) -> AppResult<Vec<Media>> {
    let rows = sqlx::query_as::<_, Media>(&format!(
        "SELECT {COLS} FROM media ORDER BY created_at DESC LIMIT 500"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get(pool: &PgPool, id: Uuid) -> AppResult<Media> {
    let row = sqlx::query_as::<_, Media>(&format!("SELECT {COLS} FROM media WHERE id = $1"))
        .bind(id)
        .fetch_one(pool)
        .await?;
    Ok(row)
}

pub async fn exists(pool: &PgPool, id: Uuid) -> AppResult<bool> {
    let found: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM media WHERE id = $1)")
        .bind(id)
        .fetch_one(pool)
        .await?;
    Ok(found)
}

pub async fn insert(
    conn: &mut PgConnection,
    id: Uuid,
    filename: &str,
    original_name: Option<&str>,
    mime: &str,
    size: i64,
    sha256: &str,
    actor: Option<Uuid>,
) -> AppResult<Media> {
    let row = sqlx::query_as::<_, Media>(&format!(
        "INSERT INTO media (id, filename, original_name, mime, size_bytes, sha256, uploaded_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING {COLS}"
    ))
    .bind(id)
    .bind(filename)
    .bind(original_name)
    .bind(mime)
    .bind(size)
    .bind(sha256)
    .bind(actor)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row)
}

pub async fn delete(conn: &mut PgConnection, id: Uuid) -> AppResult<Media> {
    let used_by_seo: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM site_settings WHERE key = 'seo' AND value->>'og_image_media' = $1)",
    )
    .bind(id.to_string())
    .fetch_one(&mut *conn)
    .await?;
    if used_by_seo {
        return Err(AppError::Conflict(
            "L'immagine è usata come immagine Open Graph".into(),
        ));
    }
    let row =
        sqlx::query_as::<_, Media>(&format!("DELETE FROM media WHERE id = $1 RETURNING {COLS}"))
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
    Ok(row)
}
