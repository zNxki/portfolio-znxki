use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{CleanContact, Contact};

const COLS: &str = "id, icon, label, value, href, position, visible, updated_at";

pub async fn list(pool: &PgPool) -> AppResult<Vec<Contact>> {
    let rows = sqlx::query_as::<_, Contact>(&format!("SELECT {COLS} FROM contacts ORDER BY position, id"))
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn get(pool: &PgPool, id: i64) -> AppResult<Contact> {
    let row = sqlx::query_as::<_, Contact>(&format!("SELECT {COLS} FROM contacts WHERE id = $1"))
        .bind(id)
        .fetch_one(pool)
        .await?;
    Ok(row)
}

pub async fn create(conn: &mut PgConnection, actor: Option<Uuid>, c: &CleanContact) -> AppResult<Contact> {
    let row = sqlx::query_as::<_, Contact>(&format!(
        "INSERT INTO contacts (icon, label, value, href, visible, position, updated_by) \
         VALUES ($1, $2, $3, $4, $5, (SELECT COALESCE(MAX(position), 0) + 1 FROM contacts), $6) \
         RETURNING {COLS}"
    ))
    .bind(&c.icon)
    .bind(&c.label)
    .bind(&c.value)
    .bind(&c.href)
    .bind(c.visible)
    .bind(actor)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row)
}

pub async fn update(conn: &mut PgConnection, actor: Option<Uuid>, id: i64, c: &CleanContact) -> AppResult<Contact> {
    let current: String = sqlx::query_scalar("SELECT icon FROM contacts WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
    if (current == "email") != (c.icon == "email") {
        return Err(AppError::Conflict(
            "Il contatto email è gestito dalle informazioni personali e non può cambiare tipo".into(),
        ));
    }
    let row = sqlx::query_as::<_, Contact>(&format!(
        "UPDATE contacts SET icon = $2, label = $3, value = $4, href = $5, visible = $6, \
                updated_at = now(), updated_by = $7 WHERE id = $1 RETURNING {COLS}"
    ))
    .bind(id)
    .bind(&c.icon)
    .bind(&c.label)
    .bind(&c.value)
    .bind(&c.href)
    .bind(c.visible)
    .bind(actor)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row)
}

pub async fn delete(conn: &mut PgConnection, id: i64) -> AppResult<Contact> {
    let current = sqlx::query_as::<_, Contact>(&format!("SELECT {COLS} FROM contacts WHERE id = $1 FOR UPDATE"))
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
    if current.icon == "email" {
        return Err(AppError::Conflict(
            "Il contatto email è obbligatorio e non può essere eliminato".into(),
        ));
    }
    sqlx::query("DELETE FROM contacts WHERE id = $1")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    Ok(current)
}