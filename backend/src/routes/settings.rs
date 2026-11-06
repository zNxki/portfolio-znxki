use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{commit, ev};
use crate::audit;
use crate::auth::extractors::{AdminUser, ClientIp};
use crate::database::{media, settings, site};
use crate::error::AppResult;
use crate::models::{GeneralSettings, SeoSettings, UiTextsInput};
use crate::routes::extract::ApiJson;
use crate::security::validation::{lang_code, Errors};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/admin/settings/general",
            get(get_general).put(put_general),
        )
        .route("/api/admin/settings/seo", get(get_seo).put(put_seo))
        .route(
            "/api/admin/settings/ui-texts",
            get(get_ui_texts).put(put_ui_texts),
        )
        .route("/api/admin/settings/ui-texts/copy", post(copy_ui_texts))
}

#[derive(Debug, Deserialize)]
struct LangQuery {
    lang: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CopyInput {
    from: String,
    to: String,
}

async fn require_language(st: &AppState, field: &str, lang: &str) -> AppResult<()> {
    let general = settings::get_general(&st.pool).await?;
    if general.languages.iter().any(|l| l == lang) {
        return Ok(());
    }
    let mut e = Errors::new();
    e.push(field, "Lingua non attiva");
    e.finish()
}

async fn get_general(
    State(st): State<AppState>,
    AdminUser(_): AdminUser,
) -> AppResult<Json<GeneralSettings>> {
    Ok(Json(settings::get_general(&st.pool).await?))
}

async fn put_general(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AdminUser(user): AdminUser,
    ApiJson(input): ApiJson<GeneralSettings>,
) -> AppResult<Json<GeneralSettings>> {
    let clean = input.validate()?;
    let mut tx = st.pool.begin().await?;
    settings::put(&mut *tx, "general", &clean, Some(user.id)).await?;
    audit::record(
        &mut *tx,
        ev("settings.general.update", &user, &ip)
            .entity("settings", "general")
            .details(json!({ "languages": clean.languages, "default_lang": clean.default_lang })),
    )
    .await?;
    commit(&st, tx).await?;
    Ok(Json(clean))
}

async fn get_seo(
    State(st): State<AppState>,
    AdminUser(_): AdminUser,
) -> AppResult<Json<SeoSettings>> {
    Ok(Json(settings::get_seo(&st.pool).await?))
}

async fn put_seo(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AdminUser(user): AdminUser,
    ApiJson(input): ApiJson<SeoSettings>,
) -> AppResult<Json<SeoSettings>> {
    let clean = input.validate()?;
    if let Some(id) = clean.og_image_media {
        if !media::exists(&st.pool, id).await? {
            let mut e = Errors::new();
            e.push("og_image_media", "Immagine inesistente");
            e.finish()?;
        }
    }
    let mut tx = st.pool.begin().await?;
    settings::put(&mut *tx, "seo", &clean, Some(user.id)).await?;
    audit::record(
        &mut *tx,
        ev("settings.seo.update", &user, &ip)
            .entity("settings", "seo")
            .details(json!({ "robots_index": clean.robots_index })),
    )
    .await?;
    commit(&st, tx).await?;
    Ok(Json(clean))
}

async fn get_ui_texts(
    State(st): State<AppState>,
    AdminUser(_): AdminUser,
    Query(q): Query<LangQuery>,
) -> AppResult<Json<Value>> {
    let mut e = Errors::new();
    let lang = lang_code(&mut e, "lang", &q.lang);
    e.finish()?;
    let items = settings::ui_texts(&st.pool, &lang).await?;
    let list: Vec<Value> = items
        .into_iter()
        .map(|(key, value)| json!({ "key": key, "value": value }))
        .collect();
    Ok(Json(json!({ "lang": lang, "items": list })))
}

async fn put_ui_texts(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AdminUser(user): AdminUser,
    ApiJson(input): ApiJson<UiTextsInput>,
) -> AppResult<StatusCode> {
    let clean = input.validate()?;
    require_language(&st, "lang", &clean.lang).await?;
    let mut e = Errors::new();
    for (key, _) in &clean.items {
        if site::is_derived_key(key) {
            e.push(
                "items.key",
                "Questa voce è gestita da Informazioni, Esperienze, Tecnologie, Progetti o SEO",
            );
        }
    }
    e.finish()?;
    let mut tx = st.pool.begin().await?;
    settings::upsert_ui_texts(&mut *tx, &clean.lang, &clean.items).await?;
    audit::record(
        &mut *tx,
        ev("settings.ui_texts.update", &user, &ip)
            .entity("ui_texts", &clean.lang)
            .details(json!({ "lang": clean.lang, "count": clean.items.len() })),
    )
    .await?;
    commit(&st, tx).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn copy_ui_texts(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AdminUser(user): AdminUser,
    ApiJson(input): ApiJson<CopyInput>,
) -> AppResult<Json<Value>> {
    let mut e = Errors::new();
    let from = lang_code(&mut e, "from", &input.from);
    let to = lang_code(&mut e, "to", &input.to);
    if from == to {
        e.push("to", "Lingua di origine e destinazione coincidono");
    }
    e.finish()?;
    require_language(&st, "from", &from).await?;
    require_language(&st, "to", &to).await?;
    let mut tx = st.pool.begin().await?;
    let res = sqlx::query(
        "INSERT INTO ui_texts (lang, key, value) \
         SELECT $2, key, value FROM ui_texts WHERE lang = $1 \
         ON CONFLICT (lang, key) DO NOTHING",
    )
    .bind(&from)
    .bind(&to)
    .execute(&mut *tx)
    .await?;
    audit::record(
        &mut *tx,
        ev("settings.ui_texts.copy", &user, &ip)
            .entity("ui_texts", &to)
            .details(json!({ "from": from, "to": to, "copied": res.rows_affected() })),
    )
    .await?;
    commit(&st, tx).await?;
    Ok(Json(json!({ "copied": res.rows_affected() })))
}
