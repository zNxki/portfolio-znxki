use sqlx::types::Json;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::error::AppResult;
use crate::models::{CleanProfile, Profile};

const COLS: &str =
    "display_name, name_parts, first_name, last_name, job_title, headline, bio_lead, \
                    bio_paragraphs, email, availability, availability_text, updated_at";

pub async fn get(pool: &PgPool) -> AppResult<Profile> {
    let row = sqlx::query_as::<_, Profile>(&format!("SELECT {COLS} FROM profile WHERE id = 1"))
        .fetch_one(pool)
        .await?;
    Ok(row)
}

pub async fn exists(pool: &PgPool) -> AppResult<bool> {
    let found: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM profile WHERE id = 1)")
        .fetch_one(pool)
        .await?;
    Ok(found)
}

pub async fn upsert(
    conn: &mut PgConnection,
    actor: Option<Uuid>,
    p: &CleanProfile,
) -> AppResult<Profile> {
    let row = sqlx::query_as::<_, Profile>(&format!(
        "INSERT INTO profile (id, display_name, name_parts, first_name, last_name, job_title, headline, \
                bio_lead, bio_paragraphs, email, availability, availability_text, updated_by) \
         VALUES (1, $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) \
         ON CONFLICT (id) DO UPDATE SET display_name = EXCLUDED.display_name, name_parts = EXCLUDED.name_parts, \
                first_name = EXCLUDED.first_name, last_name = EXCLUDED.last_name, job_title = EXCLUDED.job_title, \
                headline = EXCLUDED.headline, bio_lead = EXCLUDED.bio_lead, bio_paragraphs = EXCLUDED.bio_paragraphs, \
                email = EXCLUDED.email, availability = EXCLUDED.availability, \
                availability_text = EXCLUDED.availability_text, updated_at = now(), updated_by = EXCLUDED.updated_by \
         RETURNING {COLS}"
    ))
    .bind(&p.display_name)
    .bind(Json(&p.name_parts))
    .bind(&p.first_name)
    .bind(&p.last_name)
    .bind(Json(&p.job_title))
    .bind(Json(&p.headline))
    .bind(Json(&p.bio_lead))
    .bind(Json(&p.bio_paragraphs))
    .bind(&p.email)
    .bind(&p.availability)
    .bind(Json(&p.availability_text))
    .bind(actor)
    .fetch_one(&mut *conn)
    .await?;

    sqlx::query(
        "INSERT INTO contacts (icon, label, value, href, position, visible, updated_by) \
         VALUES ('email', 'Email', $1, $2, 0, true, $3) \
         ON CONFLICT (icon) WHERE icon = 'email' \
         DO UPDATE SET value = EXCLUDED.value, href = EXCLUDED.href, updated_at = now(), updated_by = EXCLUDED.updated_by",
    )
    .bind(&p.email)
    .bind(format!("mailto:{}", p.email))
    .bind(actor)
    .execute(&mut *conn)
    .await?;

    Ok(row)
}
