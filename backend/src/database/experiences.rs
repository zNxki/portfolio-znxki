use sqlx::types::Json;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::error::AppResult;
use crate::models::{CleanExperience, Experience};

const COLS: &str =
    "id, slug, kind, title, subtitle, role, organization, description, start_date, end_date, \
                    visible, position, updated_at";

pub async fn list(pool: &PgPool) -> AppResult<Vec<Experience>> {
    let rows = sqlx::query_as::<_, Experience>(&format!(
        "SELECT {COLS} FROM experiences ORDER BY position, id"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get(pool: &PgPool, id: i64) -> AppResult<Experience> {
    let row =
        sqlx::query_as::<_, Experience>(&format!("SELECT {COLS} FROM experiences WHERE id = $1"))
            .bind(id)
            .fetch_one(pool)
            .await?;
    Ok(row)
}

pub async fn create(
    conn: &mut PgConnection,
    actor: Option<Uuid>,
    x: &CleanExperience,
) -> AppResult<Experience> {
    let row = sqlx::query_as::<_, Experience>(&format!(
        "INSERT INTO experiences (slug, kind, title, subtitle, role, organization, description, start_date, \
                end_date, visible, position, updated_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, \
                (SELECT COALESCE(MAX(position), 0) + 1 FROM experiences), $11) \
         RETURNING {COLS}"
    ))
    .bind(&x.slug)
    .bind(&x.kind)
    .bind(Json(&x.title))
    .bind(Json(&x.subtitle))
    .bind(Json(&x.role))
    .bind(&x.organization)
    .bind(Json(&x.description))
    .bind(x.start_date)
    .bind(x.end_date)
    .bind(x.visible)
    .bind(actor)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row)
}

pub async fn update(
    conn: &mut PgConnection,
    actor: Option<Uuid>,
    id: i64,
    x: &CleanExperience,
) -> AppResult<Experience> {
    let row = sqlx::query_as::<_, Experience>(&format!(
        "UPDATE experiences SET slug = $2, kind = $3, title = $4, subtitle = $5, role = $6, organization = $7, \
                description = $8, start_date = $9, end_date = $10, visible = $11, updated_at = now(), updated_by = $12 \
         WHERE id = $1 RETURNING {COLS}"
    ))
    .bind(id)
    .bind(&x.slug)
    .bind(&x.kind)
    .bind(Json(&x.title))
    .bind(Json(&x.subtitle))
    .bind(Json(&x.role))
    .bind(&x.organization)
    .bind(Json(&x.description))
    .bind(x.start_date)
    .bind(x.end_date)
    .bind(x.visible)
    .bind(actor)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row)
}

pub async fn delete(conn: &mut PgConnection, id: i64) -> AppResult<Experience> {
    let row = sqlx::query_as::<_, Experience>(&format!(
        "DELETE FROM experiences WHERE id = $1 RETURNING {COLS}"
    ))
    .bind(id)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row)
}
