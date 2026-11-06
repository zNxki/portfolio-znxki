use sqlx::types::Json;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::error::AppResult;
use crate::models::{CleanTechnology, Technology};

const COLS: &str =
    "id, name, marquee_label, icon, level, description, in_marquee, visible, position, updated_at";

pub async fn list(pool: &PgPool) -> AppResult<Vec<Technology>> {
    let rows = sqlx::query_as::<_, Technology>(&format!(
        "SELECT {COLS} FROM technologies ORDER BY position, id"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get(pool: &PgPool, id: i64) -> AppResult<Technology> {
    let row =
        sqlx::query_as::<_, Technology>(&format!("SELECT {COLS} FROM technologies WHERE id = $1"))
            .bind(id)
            .fetch_one(pool)
            .await?;
    Ok(row)
}

pub async fn create(
    conn: &mut PgConnection,
    actor: Option<Uuid>,
    t: &CleanTechnology,
) -> AppResult<Technology> {
    let row = sqlx::query_as::<_, Technology>(&format!(
        "INSERT INTO technologies (name, marquee_label, icon, level, description, in_marquee, visible, position, updated_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, (SELECT COALESCE(MAX(position), 0) + 1 FROM technologies), $8) \
         RETURNING {COLS}"
    ))
    .bind(&t.name)
    .bind(&t.marquee_label)
    .bind(&t.icon)
    .bind(t.level)
    .bind(Json(&t.description))
    .bind(t.in_marquee)
    .bind(t.visible)
    .bind(actor)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row)
}

pub async fn update(
    conn: &mut PgConnection,
    actor: Option<Uuid>,
    id: i64,
    t: &CleanTechnology,
) -> AppResult<Technology> {
    let row = sqlx::query_as::<_, Technology>(&format!(
        "UPDATE technologies SET name = $2, marquee_label = $3, icon = $4, level = $5, description = $6, \
                in_marquee = $7, visible = $8, updated_at = now(), updated_by = $9 \
         WHERE id = $1 RETURNING {COLS}"
    ))
    .bind(id)
    .bind(&t.name)
    .bind(&t.marquee_label)
    .bind(&t.icon)
    .bind(t.level)
    .bind(Json(&t.description))
    .bind(t.in_marquee)
    .bind(t.visible)
    .bind(actor)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row)
}

pub async fn delete(conn: &mut PgConnection, id: i64) -> AppResult<Technology> {
    let row = sqlx::query_as::<_, Technology>(&format!(
        "DELETE FROM technologies WHERE id = $1 RETURNING {COLS}"
    ))
    .bind(id)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row)
}
