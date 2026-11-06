// backend/src/routes/auth.rs
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::audit::{self, Event};
use crate::auth::extractors::{AuthUser, ClientIp};
use crate::auth::{cookies, github, session, states, CurrentUser};
use crate::database::users;
use crate::error::{AppError, AppResult};
use crate::models::SessionView;
use crate::security::tokens::{ct_eq, pkce_challenge, random_token};
use crate::state::AppState;

const STATE_COOKIE_MAX_AGE: i64 = 600;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/github/login", get(login))
        .route("/auth/github/callback", get(callback))
        .route("/auth/logout", post(logout))
        .route("/api/me", get(me))
        .route("/api/me/sessions", get(my_sessions))
        .route("/api/me/sessions/{id}", delete(revoke_my_session))
}

fn append_cookie(res: &mut Response, value: &str) {
    if let Ok(v) = HeaderValue::from_str(value) {
        res.headers_mut().append(header::SET_COOKIE, v);
    }
}

async fn login(State(st): State<AppState>, ClientIp(ip): ClientIp) -> AppResult<Response> {
    let state_token = random_token();
    let verifier = random_token();
    states::create(&st.pool, &state_token, &verifier, &ip).await?;
    let url = github::authorize_url(&st.cfg, &state_token, &pkce_challenge(&verifier))?;
    let secure = st.cfg.cookie_secure();
    let mut res = Redirect::to(&url).into_response();
    append_cookie(
        &mut res,
        &cookies::build(
            cookies::state_name(secure),
            &state_token,
            STATE_COOKIE_MAX_AGE,
            secure,
        ),
    );
    Ok(res)
}

#[derive(Debug, Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

struct LoginOutcome {
    rotated: session::Rotated,
}

async fn finish_login(
    st: &AppState,
    headers: &HeaderMap,
    ip: &str,
    q: CallbackQuery,
) -> AppResult<LoginOutcome> {
    if q.error.is_some() {
        return Err(AppError::bad_request("Autorizzazione negata dal provider"));
    }
    let code = q
        .code
        .ok_or_else(|| AppError::bad_request("Parametro code mancante"))?;
    let state = q
        .state
        .ok_or_else(|| AppError::bad_request("Parametro state mancante"))?;
    if code.is_empty() || code.len() > 512 || state.is_empty() || state.len() > 128 {
        return Err(AppError::bad_request("Parametri non validi"));
    }

    let secure = st.cfg.cookie_secure();
    let cookie_state = cookies::read(headers, cookies::state_name(secure))
        .ok_or_else(|| AppError::bad_request("Cookie di stato mancante"))?;
    if !ct_eq(cookie_state.as_bytes(), state.as_bytes()) {
        return Err(AppError::bad_request("State non corrispondente"));
    }

    let verifier = states::take(&st.pool, &state)
        .await?
        .ok_or_else(|| AppError::bad_request("State non valido o scaduto"))?;

    let access_token = github::exchange_code(&st.http, &st.cfg, &code, &verifier).await?;
    let gh = github::fetch_user(&st.http, &st.cfg, &access_token).await?;

    let user = users::login_allowed(&st.pool, &gh, &st.cfg.admin_github_ids)
        .await?
        .ok_or(AppError::Forbidden)?;

    if let Some(old) = cookies::read(headers, cookies::session_name(secure)) {
        if old.len() <= 128 {
            session::revoke_by_token(&st.pool, &old).await?;
        }
    }

    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok());
    let rotated = session::create(&st.pool, &st.cfg, user.id, ip, user_agent).await?;

    audit::record_lossy(
        &st.pool,
        Event::new("auth.login")
            .actor_raw(user.id, &user.login)
            .ip(ip)
            .entity("user", user.id)
            .details(json!({ "role": user.role, "github_id": user.github_id })),
    )
    .await;
    if user.role == "ADMIN" {
        audit::record_lossy(
            &st.pool,
            Event::new("admin.login")
                .actor_raw(user.id, &user.login)
                .ip(ip),
        )
        .await;
    }

    Ok(LoginOutcome { rotated })
}

async fn callback(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Query(q): Query<CallbackQuery>,
) -> Response {
    let secure = st.cfg.cookie_secure();
    let mut res = match finish_login(&st, &headers, &ip, q).await {
        Ok(outcome) => {
            let mut r = Redirect::to("/admin/").into_response();
            append_cookie(
                &mut r,
                &cookies::build(
                    cookies::session_name(secure),
                    &outcome.rotated.token,
                    outcome.rotated.max_age,
                    secure,
                ),
            );
            r
        }
        Err(e) => {
            audit::record_lossy(
                &st.pool,
                Event::new("security.login_failed")
                    .security()
                    .ip(&ip)
                    .details(json!({ "reason": e.to_string() })),
            )
            .await;
            Redirect::to("/admin/?e=auth").into_response()
        }
    };
    append_cookie(
        &mut res,
        &cookies::clear(cookies::state_name(secure), secure),
    );
    res
}

async fn logout(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AuthUser(user): AuthUser,
) -> AppResult<Response> {
    session::revoke(&st.pool, user.session_id).await?;
    audit::record_lossy(
        &st.pool,
        Event::new("auth.logout")
            .actor(&user)
            .ip(&ip)
            .entity("session", user.session_id),
    )
    .await;
    let mut res = StatusCode::NO_CONTENT.into_response();
    append_cookie(
        &mut res,
        &cookies::clear(
            cookies::session_name(st.cfg.cookie_secure()),
            st.cfg.cookie_secure(),
        ),
    );
    Ok(res)
}

async fn me(req: Request) -> Json<Value> {
    match req.extensions().get::<CurrentUser>() {
        Some(u) => Json(json!({
            "authenticated": true,
            "user": {
                "id": u.id,
                "login": u.login,
                "display_name": u.display_name,
                "avatar_url": u.avatar_url,
                "role": u.role.as_str()
            },
            "csrf": u.csrf
        })),
        None => Json(json!({ "authenticated": false })),
    }
}

#[derive(Serialize)]
struct SessionOut {
    #[serde(flatten)]
    session: SessionView,
    current: bool,
}

async fn my_sessions(
    State(st): State<AppState>,
    AuthUser(user): AuthUser,
) -> AppResult<Json<Vec<SessionOut>>> {
    let rows = session::list_active(&st.pool, Some(user.id)).await?;
    Ok(Json(
        rows.into_iter()
            .map(|s| {
                let current = s.id == user.session_id;
                SessionOut {
                    session: s,
                    current,
                }
            })
            .collect(),
    ))
}

async fn revoke_my_session(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    if !session::revoke_owned(&st.pool, id, user.id).await? {
        return Err(AppError::NotFound);
    }
    audit::record_lossy(
        &st.pool,
        Event::new("auth.session_revoked")
            .actor(&user)
            .ip(&ip)
            .entity("session", id),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}
