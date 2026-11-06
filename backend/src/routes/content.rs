use std::collections::BTreeSet;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, put};
use axum::{Json, Router};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use super::{commit, ev};
use crate::audit;
use crate::auth::extractors::{AdminUser, ClientIp};
use crate::database::{self, contacts, experiences, profile, projects, technologies, Table};
use crate::error::AppResult;
use crate::models::{
    CleanProject, Contact, ContactInput, Experience, ExperienceInput, OrderInput, Profile,
    ProfileInput, ProjectFull, ProjectInput, Technology, TechnologyInput,
};
use crate::routes::extract::ApiJson;
use crate::security::validation::Errors;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/profile", get(get_profile).put(put_profile))
        .route(
            "/api/admin/contacts",
            get(list_contacts).post(create_contact),
        )
        .route("/api/admin/contacts/order", put(order_contacts))
        .route(
            "/api/admin/contacts/{id}",
            put(update_contact).delete(delete_contact),
        )
        .route(
            "/api/admin/technologies",
            get(list_technologies).post(create_technology),
        )
        .route("/api/admin/technologies/order", put(order_technologies))
        .route(
            "/api/admin/technologies/{id}",
            put(update_technology).delete(delete_technology),
        )
        .route(
            "/api/admin/experiences",
            get(list_experiences).post(create_experience),
        )
        .route("/api/admin/experiences/order", put(order_experiences))
        .route(
            "/api/admin/experiences/{id}",
            put(update_experience).delete(delete_experience),
        )
        .route(
            "/api/admin/projects",
            get(list_projects).post(create_project),
        )
        .route("/api/admin/projects/order", put(order_projects))
        .route(
            "/api/admin/projects/{id}",
            get(get_project).put(update_project).delete(delete_project),
        )
}

async fn get_profile(
    State(st): State<AppState>,
    AdminUser(_): AdminUser,
) -> AppResult<Json<Profile>> {
    Ok(Json(profile::get(&st.pool).await?))
}

async fn put_profile(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AdminUser(user): AdminUser,
    ApiJson(input): ApiJson<ProfileInput>,
) -> AppResult<Json<Profile>> {
    let clean = input.validate()?;
    let mut tx = st.pool.begin().await?;
    let row = profile::upsert(&mut *tx, Some(user.id), &clean).await?;
    audit::record(
        &mut *tx,
        ev("content.profile.update", &user, &ip)
            .entity("profile", 1)
            .details(json!({ "display_name": row.display_name })),
    )
    .await?;
    commit(&st, tx).await?;
    Ok(Json(row))
}

async fn list_contacts(
    State(st): State<AppState>,
    AdminUser(_): AdminUser,
) -> AppResult<Json<Vec<Contact>>> {
    Ok(Json(contacts::list(&st.pool).await?))
}

async fn create_contact(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AdminUser(user): AdminUser,
    ApiJson(input): ApiJson<ContactInput>,
) -> AppResult<(StatusCode, Json<Contact>)> {
    let current = profile::get(&st.pool).await?;
    let clean = input.validate(&current.email)?;
    let mut tx = st.pool.begin().await?;
    let row = contacts::create(&mut *tx, Some(user.id), &clean).await?;
    audit::record(
        &mut *tx,
        ev("content.contact.create", &user, &ip)
            .entity("contact", row.id)
            .details(json!({ "icon": row.icon })),
    )
    .await?;
    commit(&st, tx).await?;
    Ok((StatusCode::CREATED, Json(row)))
}

async fn update_contact(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AdminUser(user): AdminUser,
    Path(id): Path<i64>,
    ApiJson(input): ApiJson<ContactInput>,
) -> AppResult<Json<Contact>> {
    let current = profile::get(&st.pool).await?;
    let clean = input.validate(&current.email)?;
    let mut tx = st.pool.begin().await?;
    let row = contacts::update(&mut *tx, Some(user.id), id, &clean).await?;
    audit::record(
        &mut *tx,
        ev("content.contact.update", &user, &ip)
            .entity("contact", id)
            .details(json!({ "icon": row.icon })),
    )
    .await?;
    commit(&st, tx).await?;
    Ok(Json(row))
}

async fn delete_contact(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AdminUser(user): AdminUser,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    let mut tx = st.pool.begin().await?;
    let row = contacts::delete(&mut *tx, id).await?;
    audit::record(
        &mut *tx,
        ev("content.contact.delete", &user, &ip)
            .entity("contact", id)
            .details(json!({ "icon": row.icon, "label": row.label })),
    )
    .await?;
    commit(&st, tx).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn order_contacts(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AdminUser(user): AdminUser,
    ApiJson(input): ApiJson<OrderInput>,
) -> AppResult<StatusCode> {
    let ids = input.validate()?;
    let mut tx = st.pool.begin().await?;
    database::reorder(&mut *tx, Table::Contacts, &ids).await?;
    audit::record(&mut *tx, ev("content.contact.reorder", &user, &ip)).await?;
    commit(&st, tx).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_technologies(
    State(st): State<AppState>,
    AdminUser(_): AdminUser,
) -> AppResult<Json<Vec<Technology>>> {
    Ok(Json(technologies::list(&st.pool).await?))
}

async fn create_technology(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AdminUser(user): AdminUser,
    ApiJson(input): ApiJson<TechnologyInput>,
) -> AppResult<(StatusCode, Json<Technology>)> {
    let clean = input.validate()?;
    let mut tx = st.pool.begin().await?;
    let row = technologies::create(&mut *tx, Some(user.id), &clean).await?;
    audit::record(
        &mut *tx,
        ev("content.technology.create", &user, &ip)
            .entity("technology", row.id)
            .details(json!({ "name": row.name })),
    )
    .await?;
    commit(&st, tx).await?;
    Ok((StatusCode::CREATED, Json(row)))
}

async fn update_technology(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AdminUser(user): AdminUser,
    Path(id): Path<i64>,
    ApiJson(input): ApiJson<TechnologyInput>,
) -> AppResult<Json<Technology>> {
    let clean = input.validate()?;
    let mut tx = st.pool.begin().await?;
    let row = technologies::update(&mut *tx, Some(user.id), id, &clean).await?;
    audit::record(
        &mut *tx,
        ev("content.technology.update", &user, &ip)
            .entity("technology", id)
            .details(json!({ "name": row.name })),
    )
    .await?;
    commit(&st, tx).await?;
    Ok(Json(row))
}

async fn delete_technology(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AdminUser(user): AdminUser,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    let mut tx = st.pool.begin().await?;
    let row = technologies::delete(&mut *tx, id).await?;
    audit::record(
        &mut *tx,
        ev("content.technology.delete", &user, &ip)
            .entity("technology", id)
            .details(json!({ "name": row.name })),
    )
    .await?;
    commit(&st, tx).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn order_technologies(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AdminUser(user): AdminUser,
    ApiJson(input): ApiJson<OrderInput>,
) -> AppResult<StatusCode> {
    let ids = input.validate()?;
    let mut tx = st.pool.begin().await?;
    database::reorder(&mut *tx, Table::Technologies, &ids).await?;
    audit::record(&mut *tx, ev("content.technology.reorder", &user, &ip)).await?;
    commit(&st, tx).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_experiences(
    State(st): State<AppState>,
    AdminUser(_): AdminUser,
) -> AppResult<Json<Vec<Experience>>> {
    Ok(Json(experiences::list(&st.pool).await?))
}

async fn create_experience(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AdminUser(user): AdminUser,
    ApiJson(input): ApiJson<ExperienceInput>,
) -> AppResult<(StatusCode, Json<Experience>)> {
    let clean = input.validate()?;
    let mut tx = st.pool.begin().await?;
    let row = experiences::create(&mut *tx, Some(user.id), &clean).await?;
    audit::record(
        &mut *tx,
        ev("content.experience.create", &user, &ip)
            .entity("experience", row.id)
            .details(json!({ "slug": row.slug })),
    )
    .await?;
    commit(&st, tx).await?;
    Ok((StatusCode::CREATED, Json(row)))
}

async fn update_experience(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AdminUser(user): AdminUser,
    Path(id): Path<i64>,
    ApiJson(input): ApiJson<ExperienceInput>,
) -> AppResult<Json<Experience>> {
    let clean = input.validate()?;
    let mut tx = st.pool.begin().await?;
    let row = experiences::update(&mut *tx, Some(user.id), id, &clean).await?;
    audit::record(
        &mut *tx,
        ev("content.experience.update", &user, &ip)
            .entity("experience", id)
            .details(json!({ "slug": row.slug })),
    )
    .await?;
    commit(&st, tx).await?;
    Ok(Json(row))
}

async fn delete_experience(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AdminUser(user): AdminUser,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    let mut tx = st.pool.begin().await?;
    let row = experiences::delete(&mut *tx, id).await?;
    audit::record(
        &mut *tx,
        ev("content.experience.delete", &user, &ip)
            .entity("experience", id)
            .details(json!({ "slug": row.slug })),
    )
    .await?;
    commit(&st, tx).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn order_experiences(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AdminUser(user): AdminUser,
    ApiJson(input): ApiJson<OrderInput>,
) -> AppResult<StatusCode> {
    let ids = input.validate()?;
    let mut tx = st.pool.begin().await?;
    database::reorder(&mut *tx, Table::Experiences, &ids).await?;
    audit::record(&mut *tx, ev("content.experience.reorder", &user, &ip)).await?;
    commit(&st, tx).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn check_project_refs(pool: &PgPool, clean: &CleanProject) -> AppResult<()> {
    let mut e = Errors::new();
    if !clean.technology_ids.is_empty() {
        let found: i64 = sqlx::query_scalar("SELECT count(*) FROM technologies WHERE id = ANY($1)")
            .bind(clean.technology_ids.clone())
            .fetch_one(pool)
            .await?;
        if found != clean.technology_ids.len() as i64 {
            e.push("technology_ids", "Tecnologia inesistente");
        }
    }
    let media_ids: Vec<Uuid> = clean
        .images
        .iter()
        .map(|(id, _)| *id)
        .collect::<BTreeSet<Uuid>>()
        .into_iter()
        .collect();
    if !media_ids.is_empty() {
        let found: i64 = sqlx::query_scalar("SELECT count(*) FROM media WHERE id = ANY($1)")
            .bind(media_ids.clone())
            .fetch_one(pool)
            .await?;
        if found != media_ids.len() as i64 {
            e.push("images", "Immagine inesistente");
        }
    }
    e.finish()
}

async fn list_projects(
    State(st): State<AppState>,
    AdminUser(_): AdminUser,
) -> AppResult<Json<Vec<ProjectFull>>> {
    Ok(Json(projects::list_full(&st.pool, false).await?))
}

async fn get_project(
    State(st): State<AppState>,
    AdminUser(_): AdminUser,
    Path(id): Path<i64>,
) -> AppResult<Json<ProjectFull>> {
    Ok(Json(projects::get_full(&st.pool, id).await?))
}

async fn create_project(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AdminUser(user): AdminUser,
    ApiJson(input): ApiJson<ProjectInput>,
) -> AppResult<(StatusCode, Json<ProjectFull>)> {
    let clean = input.validate()?;
    check_project_refs(&st.pool, &clean).await?;
    let mut tx = st.pool.begin().await?;
    let id = projects::create(&mut *tx, Some(user.id), &clean).await?;
    audit::record(
        &mut *tx,
        ev("content.project.create", &user, &ip)
            .entity("project", id)
            .details(json!({ "slug": clean.slug, "visible": clean.visible })),
    )
    .await?;
    commit(&st, tx).await?;
    let full = projects::get_full(&st.pool, id).await?;
    Ok((StatusCode::CREATED, Json(full)))
}

async fn update_project(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AdminUser(user): AdminUser,
    Path(id): Path<i64>,
    ApiJson(input): ApiJson<ProjectInput>,
) -> AppResult<Json<ProjectFull>> {
    let clean = input.validate()?;
    check_project_refs(&st.pool, &clean).await?;
    let mut tx = st.pool.begin().await?;
    let id = projects::update(&mut *tx, Some(user.id), id, &clean).await?;
    audit::record(
        &mut *tx,
        ev("content.project.update", &user, &ip)
            .entity("project", id)
            .details(json!({ "slug": clean.slug, "visible": clean.visible })),
    )
    .await?;
    commit(&st, tx).await?;
    Ok(Json(projects::get_full(&st.pool, id).await?))
}

async fn delete_project(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AdminUser(user): AdminUser,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    let mut tx = st.pool.begin().await?;
    let (deleted, slug) = projects::delete(&mut *tx, id).await?;
    audit::record(
        &mut *tx,
        ev("content.project.delete", &user, &ip)
            .entity("project", deleted)
            .details(json!({ "slug": slug })),
    )
    .await?;
    commit(&st, tx).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn order_projects(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AdminUser(user): AdminUser,
    ApiJson(input): ApiJson<OrderInput>,
) -> AppResult<StatusCode> {
    let ids = input.validate()?;
    let mut tx = st.pool.begin().await?;
    database::reorder(&mut *tx, Table::Projects, &ids).await?;
    audit::record(&mut *tx, ev("content.project.reorder", &user, &ip)).await?;
    commit(&st, tx).await?;
    Ok(StatusCode::NO_CONTENT)
}
