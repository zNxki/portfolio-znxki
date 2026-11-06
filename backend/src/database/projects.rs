use std::collections::HashMap;

use sqlx::types::Json;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::error::AppResult;
use crate::models::{CleanProject, Project, ProjectFull, ProjectImage, ProjectImageOut};

const COLS: &str =
    "id, slug, name, title, subtitle, description, year_label, hue, status, github_url, demo_url, \
                    started_on, ended_on, visible, position, created_at, updated_at";

const IMAGE_SQL: &str =
    "SELECT pi.id, pi.project_id, pi.media_id, pi.alt, pi.position, m.filename \
                         FROM project_images pi JOIN media m ON m.id = pi.media_id \
                         ORDER BY pi.project_id, pi.position, pi.id";

fn image_out(i: ProjectImage) -> ProjectImageOut {
    ProjectImageOut {
        id: i.id,
        media_id: i.media_id,
        alt: i.alt.0,
        position: i.position,
        url: format!("/uploads/{}", i.filename),
    }
}

async fn assemble(pool: &PgPool, projects: Vec<Project>) -> AppResult<Vec<ProjectFull>> {
    let images = sqlx::query_as::<_, ProjectImage>(IMAGE_SQL)
        .fetch_all(pool)
        .await?;
    let techs: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT project_id, technology_id FROM project_technologies ORDER BY project_id, technology_id",
    )
    .fetch_all(pool)
    .await?;

    let mut images_by: HashMap<i64, Vec<ProjectImageOut>> = HashMap::new();
    for i in images {
        images_by
            .entry(i.project_id)
            .or_default()
            .push(image_out(i));
    }
    let mut techs_by: HashMap<i64, Vec<i64>> = HashMap::new();
    for (p, t) in techs {
        techs_by.entry(p).or_default().push(t);
    }

    Ok(projects
        .into_iter()
        .map(|p| {
            let id = p.id;
            ProjectFull {
                project: p,
                technology_ids: techs_by.remove(&id).unwrap_or_default(),
                images: images_by.remove(&id).unwrap_or_default(),
            }
        })
        .collect())
}

pub async fn list_plain(pool: &PgPool) -> AppResult<Vec<Project>> {
    let rows = sqlx::query_as::<_, Project>(&format!(
        "SELECT {COLS} FROM projects ORDER BY position, id"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_full(pool: &PgPool, only_visible: bool) -> AppResult<Vec<ProjectFull>> {
    let rows = sqlx::query_as::<_, Project>(&format!(
        "SELECT {COLS} FROM projects WHERE ($1::bool = false OR visible) ORDER BY position, id"
    ))
    .bind(only_visible)
    .fetch_all(pool)
    .await?;
    assemble(pool, rows).await
}

pub async fn get_full(pool: &PgPool, id: i64) -> AppResult<ProjectFull> {
    let row = sqlx::query_as::<_, Project>(&format!("SELECT {COLS} FROM projects WHERE id = $1"))
        .bind(id)
        .fetch_one(pool)
        .await?;
    let mut all = assemble(pool, vec![row]).await?;
    all.pop().ok_or(crate::error::AppError::NotFound)
}

async fn set_children(conn: &mut PgConnection, id: i64, p: &CleanProject) -> AppResult<()> {
    sqlx::query("DELETE FROM project_technologies WHERE project_id = $1")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    if !p.technology_ids.is_empty() {
        sqlx::query(
            "INSERT INTO project_technologies (project_id, technology_id) \
             SELECT $1, unnest($2::bigint[])",
        )
        .bind(id)
        .bind(p.technology_ids.clone())
        .execute(&mut *conn)
        .await?;
    }
    sqlx::query("DELETE FROM project_images WHERE project_id = $1")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    for (pos, (media_id, alt)) in p.images.iter().enumerate() {
        sqlx::query("INSERT INTO project_images (project_id, media_id, alt, position) VALUES ($1, $2, $3, $4)")
            .bind(id)
            .bind(media_id)
            .bind(Json(alt))
            .bind(pos as i32)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

pub async fn create(
    conn: &mut PgConnection,
    actor: Option<Uuid>,
    p: &CleanProject,
) -> AppResult<i64> {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO projects (slug, name, title, subtitle, description, year_label, hue, status, github_url, \
                demo_url, started_on, ended_on, visible, position, created_by, updated_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, \
                (SELECT COALESCE(MAX(position), 0) + 1 FROM projects), $14, $14) \
         RETURNING id",
    )
    .bind(&p.slug)
    .bind(&p.name)
    .bind(Json(&p.title))
    .bind(Json(&p.subtitle))
    .bind(Json(&p.description))
    .bind(&p.year_label)
    .bind(p.hue)
    .bind(&p.status)
    .bind(&p.github_url)
    .bind(&p.demo_url)
    .bind(p.started_on)
    .bind(p.ended_on)
    .bind(p.visible)
    .bind(actor)
    .fetch_one(&mut *conn)
    .await?;
    set_children(conn, id, p).await?;
    Ok(id)
}

pub async fn update(
    conn: &mut PgConnection,
    actor: Option<Uuid>,
    id: i64,
    p: &CleanProject,
) -> AppResult<i64> {
    let updated: i64 = sqlx::query_scalar(
        "UPDATE projects SET slug = $2, name = $3, title = $4, subtitle = $5, description = $6, year_label = $7, \
                hue = $8, status = $9, github_url = $10, demo_url = $11, started_on = $12, ended_on = $13, \
                visible = $14, updated_at = now(), updated_by = $15 \
         WHERE id = $1 RETURNING id",
    )
    .bind(id)
    .bind(&p.slug)
    .bind(&p.name)
    .bind(Json(&p.title))
    .bind(Json(&p.subtitle))
    .bind(Json(&p.description))
    .bind(&p.year_label)
    .bind(p.hue)
    .bind(&p.status)
    .bind(&p.github_url)
    .bind(&p.demo_url)
    .bind(p.started_on)
    .bind(p.ended_on)
    .bind(p.visible)
    .bind(actor)
    .fetch_one(&mut *conn)
    .await?;
    set_children(conn, updated, p).await?;
    Ok(updated)
}

pub async fn delete(conn: &mut PgConnection, id: i64) -> AppResult<(i64, String)> {
    let row: (i64, String) =
        sqlx::query_as("DELETE FROM projects WHERE id = $1 RETURNING id, slug")
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
    Ok(row)
}
