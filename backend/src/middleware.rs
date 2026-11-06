use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_SECURITY_POLICY, ORIGIN, REFERER, REFERRER_POLICY,
    STRICT_TRANSPORT_SECURITY, X_CONTENT_TYPE_OPTIONS, X_FRAME_OPTIONS,
};
use axum::http::{HeaderName, HeaderValue, Method};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use uuid::Uuid;

use crate::audit::{self, Event};
use crate::auth::cookies;
use crate::auth::extractors::ClientIp;
use crate::auth::{session, CurrentUser};
use crate::error::AppError;
use crate::security::headers::API_CSP;
use crate::security::tokens::ct_eq;
use crate::state::AppState;

fn client_ip(req: &Request, trust_proxy: bool) -> String {
    if trust_proxy {
        if let Some(v) = req.headers().get("x-real-ip").and_then(|v| v.to_str().ok()) {
            if let Ok(ip) = v.trim().parse::<IpAddr>() {
                return ip.to_string();
            }
        }
    }
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

pub async fn request_context(State(st): State<AppState>, mut req: Request, next: Next) -> Response {
    let id = Uuid::new_v4().to_string();
    let ip = client_ip(&req, st.cfg.trust_proxy);
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    req.extensions_mut().insert(ClientIp(ip.clone()));
    let start = Instant::now();

    let mut res = next.run(req).await;

    tracing::info!(
        request_id = %id,
        method = %method,
        path = %path,
        status = res.status().as_u16(),
        elapsed_ms = start.elapsed().as_millis() as u64,
        ip = %ip,
        "request"
    );
    if let Ok(v) = HeaderValue::from_str(&id) {
        res.headers_mut()
            .insert(HeaderName::from_static("x-request-id"), v);
    }
    res
}

pub async fn security_headers(State(st): State<AppState>, req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let mut res = next.run(req).await;
    let sensitive =
        path.starts_with("/api/") || path.starts_with("/auth/") || path.starts_with("/admin");

    let csp = if path.starts_with("/admin") {
        st.csp_admin.clone()
    } else if path.starts_with("/api/")
        || path.starts_with("/auth/")
        || path.starts_with("/uploads/")
    {
        HeaderValue::from_static(API_CSP)
    } else {
        st.csp_public.clone()
    };

    let headers = res.headers_mut();
    headers.insert(CONTENT_SECURITY_POLICY, csp);
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        REFERRER_POLICY,
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    headers.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("camera=(), microphone=(), geolocation=(), payment=(), usb=()"),
    );
    headers.insert(
        HeaderName::from_static("cross-origin-opener-policy"),
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        HeaderName::from_static("cross-origin-resource-policy"),
        HeaderValue::from_static("same-origin"),
    );
    if st.cfg.cookie_secure() {
        headers.insert(
            STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=63072000; includeSubDomains"),
        );
    }
    if sensitive && !headers.contains_key(CACHE_CONTROL) {
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    res
}

pub async fn session_layer(State(st): State<AppState>, mut req: Request, next: Next) -> Response {
    let path = req.uri().path();
    let needs_session =
        path.starts_with("/api/") || path.starts_with("/auth/") || path.starts_with("/admin");
    let secure = st.cfg.cookie_secure();
    let mut set_cookie: Option<String> = None;

    if needs_session {
        if let Some(token) = cookies::read(req.headers(), cookies::session_name(secure)) {
            if token.len() > 128 {
                set_cookie = Some(cookies::clear(cookies::session_name(secure), secure));
            } else {
                match session::resolve(&st.pool, &st.cfg, &token).await {
                    Ok(Some(resolved)) => {
                        req.extensions_mut().insert(resolved.user);
                        if let Some(r) = resolved.rotated {
                            set_cookie = Some(cookies::build(
                                cookies::session_name(secure),
                                &r.token,
                                r.max_age,
                                secure,
                            ));
                        }
                    }
                    Ok(None) => {
                        set_cookie = Some(cookies::clear(cookies::session_name(secure), secure));
                    }
                    Err(e) => return e.into_response(),
                }
            }
        }
    }

    let mut res = next.run(req).await;
    if let Some(c) = set_cookie {
        if let Ok(v) = HeaderValue::from_str(&c) {
            res.headers_mut().append(axum::http::header::SET_COOKIE, v);
        }
    }
    res
}

pub async fn rate_limit(State(st): State<AppState>, req: Request, next: Next) -> Response {
    let path = req.uri().path();
    let (scope, limit) = if path.starts_with("/auth/") {
        ("auth", st.cfg.rate_auth_per_min)
    } else if path.starts_with("/api/") {
        ("api", st.cfg.rate_admin_per_min)
    } else {
        ("pub", st.cfg.rate_public_per_min)
    };
    let ip = req
        .extensions()
        .get::<ClientIp>()
        .map(|c| c.0.clone())
        .unwrap_or_default();
    let user_id = req.extensions().get::<CurrentUser>().map(|u| u.id);
    let window = Duration::from_secs(60);

    if let Err(retry_after) = st.limiter.check(&format!("{scope}:ip:{ip}"), limit, window) {
        tracing::warn!(scope, ip = %ip, "rate limit per IP superato");
        return AppError::TooManyRequests { retry_after }.into_response();
    }
    if let Some(id) = user_id {
        if let Err(retry_after) = st
            .limiter
            .check(&format!("{scope}:user:{id}"), limit, window)
        {
            tracing::warn!(scope, user = %id, "rate limit per utente superato");
            return AppError::TooManyRequests { retry_after }.into_response();
        }
    }
    next.run(req).await
}

fn origin_allowed(req: &Request, expected: &str) -> bool {
    if let Some(origin) = req.headers().get(ORIGIN).and_then(|v| v.to_str().ok()) {
        return origin == expected;
    }
    if let Some(referer) = req.headers().get(REFERER).and_then(|v| v.to_str().ok()) {
        return referer == expected || referer.starts_with(&format!("{expected}/"));
    }
    false
}

pub async fn csrf(State(st): State<AppState>, req: Request, next: Next) -> Response {
    let method = req.method().clone();
    if method == Method::GET || method == Method::HEAD || method == Method::OPTIONS {
        return next.run(req).await;
    }

    let user = req.extensions().get::<CurrentUser>().cloned();
    let ip = req
        .extensions()
        .get::<ClientIp>()
        .map(|c| c.0.clone())
        .unwrap_or_default();

    let origin_ok = origin_allowed(&req, &st.cfg.origin());
    let token_ok = match &user {
        Some(u) => req
            .headers()
            .get("x-csrf-token")
            .and_then(|v| v.to_str().ok())
            .map(|t| ct_eq(t.as_bytes(), u.csrf.as_bytes()))
            .unwrap_or(false),
        None => true,
    };

    if !origin_ok || !token_ok {
        let mut ev = Event::new("security.csrf_blocked")
            .security()
            .ip(&ip)
            .details(serde_json::json!({
                "path": req.uri().path(),
                "method": method.as_str(),
                "origin_ok": origin_ok,
                "token_ok": token_ok
            }));
        if let Some(u) = &user {
            ev = ev.actor(u);
        }
        audit::record_lossy(&st.pool, ev).await;
        return AppError::Forbidden.into_response();
    }
    next.run(req).await
}
