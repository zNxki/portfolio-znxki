//! Test d'integrazione: backend reale + PostgreSQL reale + GitHub simulato in locale.
//!
//! Richiedono un'istanza PostgreSQL con un ruolo che possa creare database.
//! `TEST_DATABASE_URL` (default `postgres://pf:pf@localhost:5432/postgres`).
//! Ogni test crea e poi elimina un database dedicato.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, Method, Request, StatusCode};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use http_body_util::BodyExt;
use portfolio_backend::config::Config;
use portfolio_backend::database::{seed, users};
use portfolio_backend::{app, auth::session};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tower::ServiceExt;
use url::Url;
use uuid::Uuid;

/* ───────────── GitHub simulato ───────────── */

#[derive(Default)]
struct GhRec {
    verifiers: Vec<String>,
}

async fn gh_token(
    State(rec): State<Arc<Mutex<GhRec>>>,
    Form(f): Form<HashMap<String, String>>,
) -> Json<Value> {
    let code = f.get("code").cloned().unwrap_or_default();
    if let Ok(mut g) = rec.lock() {
        g.verifiers.push(f.get("code_verifier").cloned().unwrap_or_default());
    }
    Json(json!({ "access_token": format!("tok-{code}"), "token_type": "bearer" }))
}

async fn gh_user(headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
    let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("");
    match auth.strip_prefix("Bearer tok-") {
        Some("admin") => Ok(Json(json!({
            "id": 1001, "login": "owner", "name": "The Owner", "avatar_url": "https://avatars.example/1.png"
        }))),
        Some("intruder") => Ok(Json(json!({ "id": 2002, "login": "intruder", "name": null, "avatar_url": null }))),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

/* ───────────── harness ───────────── */

struct Ctx {
    router: Router,
    pool: PgPool,
    rec: Arc<Mutex<GhRec>>,
    root: PgPool,
    db: String,
    origin: String,
    secure: bool,
    cfg: Config,
}

fn base_url() -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| "postgres://pf:pf@localhost:5432/postgres".into())
}

async fn setup(tweak: impl FnOnce(&mut Config)) -> Ctx {
    let root = PgPoolOptions::new().max_connections(2).connect(&base_url()).await.expect("PostgreSQL non raggiungibile");
    let db = format!("pf_test_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE DATABASE {db}")).execute(&root).await.expect("create database");
    let mut url = Url::parse(&base_url()).expect("url");
    url.set_path(&db);

    let rec = Arc::new(Mutex::new(GhRec::default()));
    let gh = Router::new()
        .route("/login/oauth/access_token", post(gh_token))
        .route("/user", get(gh_user))
        .with_state(rec.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let gh_addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, gh).await;
    });
    let gh_base = format!("http://{gh_addr}");

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let uploads = std::env::temp_dir().join(format!("pf-uploads-{}", Uuid::new_v4()));
    let mut cfg = Config {
        bind_addr: "127.0.0.1:0".parse().expect("addr"),
        public_url: Url::parse("http://localhost:8080").expect("url"),
        database_url: url.to_string(),
        migration_database_url: None,
        db_max_connections: 5,
        github_client_id: "cid".into(),
        github_client_secret: "s3cret-client-secret".into(),
        github_authorize_url: format!("{gh_base}/login/oauth/authorize"),
        github_token_url: format!("{gh_base}/login/oauth/access_token"),
        github_api_url: gh_base,
        app_secret: b"0123456789abcdef0123456789abcdef".to_vec(),
        admin_github_ids: vec![1001],
        frontend_dir: manifest.join(".."),
        admin_dir: manifest.join("admin"),
        upload_dir: uploads,
        trust_proxy: true,
        session_idle_secs: 1800,
        session_absolute_secs: 43200,
        session_rotate_secs: 900,
        rate_public_per_min: 100_000,
        rate_auth_per_min: 100_000,
        rate_admin_per_min: 100_000,
        max_json_bytes: 1_048_576,
        max_upload_bytes: 1_048_576,
    };
    tweak(&mut cfg);
    let origin = cfg.origin();
    let secure = cfg.cookie_secure();

    app::migrate(&cfg).await.expect("migrazioni");
    let pool = app::connect(&cfg).await.expect("pool");
    seed::run(&pool, &cfg).await.expect("seed");
    users::sync_admins(&pool, &cfg.admin_github_ids).await.expect("sync");
    let st = app::build_state(cfg.clone(), pool.clone()).await.expect("stato");
    Ctx { router: app::build_router(st), pool, rec, root, db, origin, secure, cfg }
}

impl Ctx {
    async fn finish(self) {
        self.pool.close().await;
        let _ = sqlx::query(&format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", self.db)).execute(&self.root).await;
    }
    fn session_cookie(&self) -> &'static str {
        if self.secure { "__Host-session" } else { "session" }
    }
    fn state_cookie(&self) -> &'static str {
        if self.secure { "__Host-oauth_state" } else { "oauth_state" }
    }
}

struct Rq {
    method: Method,
    path: String,
    cookie: Option<String>,
    csrf: Option<String>,
    origin: Option<String>,
    body: Option<(String, Vec<u8>)>,
    ip: String,
}

impl Rq {
    fn new(method: Method, path: &str, ctx: &Ctx) -> Rq {
        let origin = if method == Method::GET { None } else { Some(ctx.origin.clone()) };
        Rq { method, path: path.to_string(), cookie: None, csrf: None, origin, body: None, ip: "10.0.0.1".into() }
    }
    fn cookie(mut self, c: &str) -> Rq { self.cookie = Some(c.to_string()); self }
    fn csrf(mut self, c: &str) -> Rq { self.csrf = Some(c.to_string()); self }
    fn origin(mut self, o: Option<&str>) -> Rq { self.origin = o.map(str::to_string); self }
    fn ip(mut self, ip: &str) -> Rq { self.ip = ip.to_string(); self }
    fn json(mut self, v: Value) -> Rq { self.body = Some(("application/json".into(), v.to_string().into_bytes())); self }
    fn raw(mut self, ct: &str, b: Vec<u8>) -> Rq { self.body = Some((ct.into(), b)); self }
}

struct Resp {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl Resp {
    fn json(&self) -> Value { serde_json::from_slice(&self.body).unwrap_or(Value::Null) }
    fn text(&self) -> String { String::from_utf8_lossy(&self.body).to_string() }
    fn h(&self, name: &str) -> String {
        self.headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or("").to_string()
    }
    fn set_cookies(&self) -> Vec<String> {
        self.headers.get_all(header::SET_COOKIE).iter().filter_map(|v| v.to_str().ok()).map(str::to_string).collect()
    }
    /// Valore (non vuoto) del cookie impostato dalla risposta.
    fn cookie_value(&self, name: &str) -> Option<String> {
        self.set_cookies().iter().find_map(|c| {
            let first = c.split(';').next()?;
            let (k, v) = first.split_once('=')?;
            (k == name && !v.is_empty()).then(|| v.to_string())
        })
    }
}

async fn call(ctx: &Ctx, rq: Rq) -> Resp {
    let mut b = Request::builder().method(rq.method.clone()).uri(&rq.path);
    if let Some(c) = &rq.cookie { b = b.header(header::COOKIE, c); }
    if let Some(c) = &rq.csrf { b = b.header("x-csrf-token", c); }
    if let Some(o) = &rq.origin { b = b.header(header::ORIGIN, o); }
    if let Some((ct, _)) = &rq.body { b = b.header(header::CONTENT_TYPE, ct); }
    let body = rq.body.map(|(_, v)| Body::from(v)).unwrap_or_else(Body::empty);
    let mut req = b.body(body).expect("richiesta");
    req.extensions_mut().insert(ConnectInfo(SocketAddr::new(rq.ip.parse().expect("ip"), 40000)));
    // anche l'header x-real-ip (TRUST_PROXY=true nei test)
    req.headers_mut().insert("x-real-ip", rq.ip.parse().expect("hdr"));
    let res = ctx.router.clone().oneshot(req).await.expect("risposta");
    let status = res.status();
    let headers = res.headers().clone();
    let body = res.into_body().collect().await.expect("body").to_bytes().to_vec();
    Resp { status, headers, body }
}

struct Sess {
    cookie: String,
    token: String,
    csrf: String,
}

impl Sess {
    fn get(&self, ctx: &Ctx, path: &str) -> Rq { Rq::new(Method::GET, path, ctx).cookie(&self.cookie) }
    fn write(&self, ctx: &Ctx, m: Method, path: &str) -> Rq { Rq::new(m, path, ctx).cookie(&self.cookie).csrf(&self.csrf) }
}

struct LoginTrace {
    cb: Resp,
    state: String,
    challenge: String,
    method: String,
    authorize: Url,
}

async fn do_login(ctx: &Ctx, who: &str) -> LoginTrace {
    let r = call(ctx, Rq::new(Method::GET, "/auth/github/login", ctx)).await;
    assert!(r.status.is_redirection(), "login deve reindirizzare: {}", r.status);
    let authorize = Url::parse(&r.h("location")).expect("location");
    let q: HashMap<String, String> = authorize.query_pairs().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let state = q.get("state").cloned().unwrap_or_default();
    assert_eq!(r.cookie_value(ctx.state_cookie()).as_deref(), Some(state.as_str()), "cookie di stato");
    let cb = call(
        ctx,
        Rq::new(Method::GET, &format!("/auth/github/callback?code={who}&state={state}"), ctx)
            .cookie(&format!("{}={state}", ctx.state_cookie())),
    )
    .await;
    LoginTrace {
        cb,
        state,
        challenge: q.get("code_challenge").cloned().unwrap_or_default(),
        method: q.get("code_challenge_method").cloned().unwrap_or_default(),
        authorize,
    }
}

async fn admin_session(ctx: &Ctx) -> Sess {
    let t = do_login(ctx, "admin").await;
    assert_eq!(t.cb.h("location"), "/admin/", "login admin riuscito");
    let token = t.cb.cookie_value(ctx.session_cookie()).expect("cookie di sessione");
    let cookie = format!("{}={token}", ctx.session_cookie());
    let me = call(ctx, Rq::new(Method::GET, "/api/me", ctx).cookie(&cookie)).await.json();
    assert_eq!(me["authenticated"], true);
    Sess { cookie, token, csrf: me["csrf"].as_str().unwrap_or_default().to_string() }
}

fn png() -> Vec<u8> {
    let mut v = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    v.extend_from_slice(&[0u8; 64]);
    v
}

fn multipart(name: &str, bytes: &[u8]) -> (String, Vec<u8>) {
    let b = "XBOUNDARYX";
    let mut body = format!("--{b}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").into_bytes();
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{b}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={b}"), body)
}

/* ───────────── OAuth ───────────── */

#[tokio::test]
async fn oauth_flow_uses_state_pkce_and_hashed_sessions() {
    let ctx = setup(|_| {}).await;
    let t = do_login(&ctx, "admin").await;

    // authorize URL: client_id, state, PKCE S256, nessun secret
    assert_eq!(t.method, "S256");
    let au = t.authorize.to_string();
    assert!(au.contains("client_id=cid") && !au.contains("s3cret-client-secret"));
    assert!(t.state.len() >= 32);

    // il code_verifier inviato a GitHub corrisponde al challenge
    let verifier = ctx.rec.lock().expect("lock").verifiers.first().cloned().unwrap_or_default();
    assert_eq!(URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())), t.challenge);

    // callback: sessione impostata, cookie HttpOnly + SameSite
    assert_eq!(t.cb.h("location"), "/admin/");
    let sc = t.cb.set_cookies().into_iter().find(|c| c.starts_with("session=")).expect("set-cookie sessione");
    assert!(sc.contains("HttpOnly") && sc.contains("SameSite=Lax") && sc.contains("Path=/"));
    // il cookie di stato viene cancellato
    assert!(t.cb.set_cookies().iter().any(|c| c.starts_with("oauth_state=;")));

    // il DB conserva solo l'hash del token
    let token = t.cb.cookie_value("session").expect("token");
    let hash: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM sessions LIMIT 1").fetch_one(&ctx.pool).await.expect("hash");
    assert_eq!(hash.len(), 32);
    assert_ne!(hash, token.as_bytes());
    assert_eq!(hash, Sha256::digest(token.as_bytes()).to_vec());

    // l'utente è identificato dal GitHub ID ed è ADMIN
    let (gid, role): (i64, String) = sqlx::query_as("SELECT github_id, role FROM users").fetch_one(&ctx.pool).await.expect("user");
    assert_eq!((gid, role.as_str()), (1001, "ADMIN"));

    // nessun token/cookie/segreto finisce nell'audit log
    let dump: String = sqlx::query_scalar("SELECT coalesce(string_agg(action || coalesce(details::text,'') || coalesce(ip,''), ' '), '') FROM audit_log")
        .fetch_one(&ctx.pool).await.expect("audit");
    assert!(!dump.contains(&token) && !dump.contains("tok-admin") && !dump.contains("s3cret-client-secret"));
    assert!(dump.contains("auth.login") && dump.contains("admin.login"));
    ctx.finish().await;
}

#[tokio::test]
async fn oauth_rejects_bad_state_missing_cookie_and_replay() {
    let ctx = setup(|_| {}).await;
    let r = call(&ctx, Rq::new(Method::GET, "/auth/github/login", &ctx)).await;
    let state = Url::parse(&r.h("location")).expect("u").query_pairs().find(|(k, _)| k == "state").map(|(_, v)| v.to_string()).unwrap_or_default();

    // senza cookie di stato
    let a = call(&ctx, Rq::new(Method::GET, &format!("/auth/github/callback?code=admin&state={state}"), &ctx)).await;
    assert_eq!(a.h("location"), "/admin/?e=auth");
    assert!(a.cookie_value("session").is_none());

    // state diverso dal cookie
    let b = call(&ctx, Rq::new(Method::GET, &format!("/auth/github/callback?code=admin&state={state}"), &ctx).cookie("oauth_state=altro")).await;
    assert_eq!(b.h("location"), "/admin/?e=auth");

    // state valido: funziona una sola volta
    let ok = call(&ctx, Rq::new(Method::GET, &format!("/auth/github/callback?code=admin&state={state}"), &ctx).cookie(&format!("oauth_state={state}"))).await;
    assert_eq!(ok.h("location"), "/admin/");
    let replay = call(&ctx, Rq::new(Method::GET, &format!("/auth/github/callback?code=admin&state={state}"), &ctx).cookie(&format!("oauth_state={state}"))).await;
    assert_eq!(replay.h("location"), "/admin/?e=auth");
    assert!(replay.cookie_value("session").is_none());

    // errore dal provider
    let denied = call(&ctx, Rq::new(Method::GET, "/auth/github/callback?error=access_denied", &ctx)).await;
    assert_eq!(denied.h("location"), "/admin/?e=auth");

    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = 'security.login_failed'").fetch_one(&ctx.pool).await.expect("n");
    assert!(n >= 3, "eventi di sicurezza registrati: {n}");
    ctx.finish().await;
}

#[tokio::test]
async fn only_allowlisted_github_ids_get_a_session() {
    let ctx = setup(|_| {}).await;
    let t = do_login(&ctx, "intruder").await;
    assert_eq!(t.cb.h("location"), "/admin/?e=auth");
    assert!(t.cb.cookie_value("session").is_none());
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE github_id = 2002").fetch_one(&ctx.pool).await.expect("n");
    assert_eq!(n, 0, "nessun utente creato per account non autorizzati");

    // un admin rimosso dall'allowlist viene declassato e le sue sessioni revocate
    let s = admin_session(&ctx).await;
    assert_eq!(call(&ctx, s.get(&ctx, "/api/admin/dashboard")).await.status, StatusCode::OK);
    users::sync_admins(&ctx.pool, &[4004]).await.expect("sync");
    assert_eq!(call(&ctx, s.get(&ctx, "/api/admin/dashboard")).await.status, StatusCode::UNAUTHORIZED);
    ctx.finish().await;
}

#[test]
fn config_requires_admin_github_ids() {
    for (k, v) in [
        ("PUBLIC_URL", "https://example.test"), ("APP_SECRET", "0123456789abcdef0123456789abcdef"),
        ("DATABASE_URL", "postgres://x"), ("GITHUB_CLIENT_ID", "a"), ("GITHUB_CLIENT_SECRET", "b"),
    ] {
        std::env::set_var(k, v);
    }
    std::env::set_var("ADMIN_GITHUB_IDS", "");
    assert!(Config::from_env().is_err(), "senza ADMIN_GITHUB_IDS il server non deve partire");
    std::env::set_var("ADMIN_GITHUB_IDS", "abc");
    assert!(Config::from_env().is_err());
    std::env::set_var("ADMIN_GITHUB_IDS", "1001, 1002");
    assert_eq!(Config::from_env().map(|c| c.admin_github_ids).unwrap_or_default(), vec![1001, 1002]);
}

/* ───────────── sessioni ───────────── */

#[tokio::test]
async fn session_logout_idle_absolute_rotation_and_revocation() {
    let ctx = setup(|_| {}).await;

    // logout effettivo lato server
    let s = admin_session(&ctx).await;
    let out = call(&ctx, s.write(&ctx, Method::POST, "/auth/logout")).await;
    assert_eq!(out.status, StatusCode::NO_CONTENT);
    assert!(out.set_cookies().iter().any(|c| c.starts_with("session=;") || c.contains("Max-Age=0")));
    let me = call(&ctx, Rq::new(Method::GET, "/api/me", &ctx).cookie(&s.cookie)).await.json();
    assert_eq!(me["authenticated"], false, "il cookie vecchio non deve più funzionare");
    let revoked: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE revoked_at IS NOT NULL").fetch_one(&ctx.pool).await.expect("n");
    assert_eq!(revoked, 1);

    // scadenza per inattività
    let s = admin_session(&ctx).await;
    sqlx::query("UPDATE sessions SET last_seen_at = now() - interval '2 hours' WHERE revoked_at IS NULL").execute(&ctx.pool).await.expect("upd");
    assert_eq!(call(&ctx, s.get(&ctx, "/api/admin/dashboard")).await.status, StatusCode::UNAUTHORIZED);

    // scadenza assoluta
    let s = admin_session(&ctx).await;
    sqlx::query("UPDATE sessions SET expires_at = now() - interval '1 second' WHERE revoked_at IS NULL AND last_seen_at > now() - interval '1 minute'").execute(&ctx.pool).await.expect("upd");
    assert_eq!(call(&ctx, s.get(&ctx, "/api/admin/dashboard")).await.status, StatusCode::UNAUTHORIZED);

    // rotazione: nuovo token, stesso CSRF, vecchio token valido solo nella breve finestra di grazia
    let s = admin_session(&ctx).await;
    sqlx::query("UPDATE sessions SET rotated_at = now() - interval '1 hour' WHERE revoked_at IS NULL AND expires_at > now()").execute(&ctx.pool).await.expect("upd");
    let r = call(&ctx, Rq::new(Method::GET, "/api/me", &ctx).cookie(&s.cookie)).await;
    let new_token = r.cookie_value("session").expect("cookie ruotato");
    assert_ne!(new_token, s.token);
    assert_eq!(r.json()["csrf"].as_str().unwrap_or(""), s.csrf);
    let r2 = call(&ctx, Rq::new(Method::GET, "/api/me", &ctx).cookie(&format!("session={new_token}"))).await;
    assert_eq!(r2.json()["authenticated"], true);
    sqlx::query("UPDATE sessions SET rotated_at = now() - interval '5 minutes' WHERE revoked_at IS NULL AND expires_at > now()").execute(&ctx.pool).await.expect("upd");
    let old = call(&ctx, Rq::new(Method::GET, "/api/me", &ctx).cookie(&s.cookie)).await.json();
    assert_eq!(old["authenticated"], false, "il token precedente scade dopo la finestra di grazia");

    // revoca di una sessione da parte dell'admin
    let a = admin_session(&ctx).await;
    let b = admin_session(&ctx).await;
    let list = call(&ctx, a.get(&ctx, "/api/admin/sessions")).await.json();
    assert!(list.as_array().map(|l| l.len()).unwrap_or(0) >= 2);
    let mine = call(&ctx, b.get(&ctx, "/api/me/sessions")).await.json();
    let b_id = mine.as_array().and_then(|l| l.iter().find(|s| s["current"] == true)).and_then(|s| s["id"].as_str()).unwrap_or("").to_string();
    let del = call(&ctx, a.write(&ctx, Method::DELETE, &format!("/api/admin/sessions/{b_id}"))).await;
    assert_eq!(del.status, StatusCode::NO_CONTENT);
    assert_eq!(call(&ctx, b.get(&ctx, "/api/admin/dashboard")).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(call(&ctx, a.get(&ctx, "/api/admin/dashboard")).await.status, StatusCode::OK);
    ctx.finish().await;
}

#[tokio::test]
async fn https_deployment_uses_host_prefixed_secure_cookies_and_hsts() {
    let ctx = setup(|c| c.public_url = Url::parse("https://example.test").expect("url")).await;
    let t = do_login(&ctx, "admin").await;
    let sc = t.cb.set_cookies().into_iter().find(|c| c.starts_with("__Host-session=")).expect("cookie __Host-");
    assert!(sc.contains("Secure") && sc.contains("HttpOnly") && sc.contains("SameSite=Lax"));
    let h = call(&ctx, Rq::new(Method::GET, "/healthz", &ctx)).await;
    assert!(h.h("strict-transport-security").contains("max-age=63072000"));
    ctx.finish().await;
}

/* ───────────── autorizzazioni e CSRF ───────────── */

const ADMIN_GETS: [&str; 13] = [
    "/api/admin/dashboard", "/api/admin/profile", "/api/admin/contacts", "/api/admin/technologies",
    "/api/admin/experiences", "/api/admin/projects", "/api/admin/media", "/api/admin/settings/general",
    "/api/admin/settings/seo", "/api/admin/settings/ui-texts?lang=it", "/api/admin/users",
    "/api/admin/sessions", "/api/admin/audit",
];

#[tokio::test]
async fn admin_api_requires_authentication_and_admin_role() {
    let ctx = setup(|_| {}).await;
    for p in ADMIN_GETS {
        let r = call(&ctx, Rq::new(Method::GET, p, &ctx)).await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{p} senza sessione");
    }
    let w = call(&ctx, Rq::new(Method::POST, "/api/admin/technologies", &ctx).json(json!({"name":"x","level":1}))).await;
    assert_eq!(w.status, StatusCode::UNAUTHORIZED);

    // utente con ruolo USER: autenticato ma non autorizzato
    let uid: Uuid = sqlx::query_scalar("INSERT INTO users (github_id, login, role) VALUES (3003, 'plain', 'USER') RETURNING id").fetch_one(&ctx.pool).await.expect("user");
    let tok = session::create(&ctx.pool, &ctx.cfg, uid, "10.0.0.9", Some("test")).await.expect("session");
    let cookie = format!("session={}", tok.token);
    let me = call(&ctx, Rq::new(Method::GET, "/api/me", &ctx).cookie(&cookie)).await.json();
    assert_eq!(me["user"]["role"], "USER");
    for p in ADMIN_GETS {
        let r = call(&ctx, Rq::new(Method::GET, p, &ctx).cookie(&cookie)).await;
        assert_eq!(r.status, StatusCode::FORBIDDEN, "{p} con ruolo USER");
    }
    let csrf = me["csrf"].as_str().unwrap_or("").to_string();
    let w = call(&ctx, Rq::new(Method::POST, "/api/admin/technologies", &ctx).cookie(&cookie).csrf(&csrf).json(json!({"name":"x","level":1}))).await;
    assert_eq!(w.status, StatusCode::FORBIDDEN);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE severity = 'security'").fetch_one(&ctx.pool).await.expect("n");
    assert!(n >= 1, "i tentativi non autorizzati sono registrati");
    ctx.finish().await;
}

#[tokio::test]
async fn csrf_and_origin_are_enforced_on_writes() {
    let ctx = setup(|_| {}).await;
    let s = admin_session(&ctx).await;
    let body = json!({"name":"Zig","level":50});

    let no_token = call(&ctx, Rq::new(Method::POST, "/api/admin/technologies", &ctx).cookie(&s.cookie).json(body.clone())).await;
    assert_eq!(no_token.status, StatusCode::FORBIDDEN, "manca il token CSRF");
    let bad_token = call(&ctx, Rq::new(Method::POST, "/api/admin/technologies", &ctx).cookie(&s.cookie).csrf("sbagliato").json(body.clone())).await;
    assert_eq!(bad_token.status, StatusCode::FORBIDDEN);
    let evil = call(&ctx, s.write(&ctx, Method::POST, "/api/admin/technologies").origin(Some("https://evil.example")).json(body.clone())).await;
    assert_eq!(evil.status, StatusCode::FORBIDDEN, "origine esterna");
    let none = call(&ctx, s.write(&ctx, Method::POST, "/api/admin/technologies").origin(None).json(body.clone())).await;
    assert_eq!(none.status, StatusCode::FORBIDDEN, "né Origin né Referer");
    let ok = call(&ctx, s.write(&ctx, Method::POST, "/api/admin/technologies").json(body)).await;
    assert_eq!(ok.status, StatusCode::CREATED);

    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = 'security.csrf_blocked'").fetch_one(&ctx.pool).await.expect("n");
    assert_eq!(n, 4);
    ctx.finish().await;
}

#[tokio::test]
async fn privileges_cannot_be_changed_through_the_api() {
    let ctx = setup(|_| {}).await;
    let s = admin_session(&ctx).await;
    let mut p = call(&ctx, s.get(&ctx, "/api/admin/profile")).await.json();
    assert!(p.is_object());
    // campi sconosciuti (es. "role") vengono rifiutati
    let obj = p.as_object_mut().expect("obj");
    obj.remove("updated_at");
    obj.insert("role".into(), json!("ADMIN"));
    let r = call(&ctx, s.write(&ctx, Method::PUT, "/api/admin/profile").json(p)).await;
    assert!(r.status.is_client_error(), "stato {}", r.status);
    // non esistono endpoint per modificare i ruoli
    for (m, p) in [(Method::PUT, "/api/admin/users"), (Method::POST, "/api/admin/users"), (Method::PUT, "/api/me")] {
        let r = call(&ctx, s.write(&ctx, m, p).json(json!({"role":"ADMIN"}))).await;
        assert!(r.status == StatusCode::NOT_FOUND || r.status == StatusCode::METHOD_NOT_ALLOWED, "{p}: {}", r.status);
    }
    ctx.finish().await;
}

/* ───────────── contenuti ───────────── */

#[tokio::test]
async fn content_changes_are_validated_audited_and_reflected_publicly() {
    let ctx = setup(|_| {}).await;
    let s = admin_session(&ctx).await;

    // tecnologia + progetto
    let tech = call(&ctx, s.write(&ctx, Method::POST, "/api/admin/technologies").json(json!({"name":"Rust","level":80,"description":{"en":"systems"}}))).await;
    assert_eq!(tech.status, StatusCode::CREATED);
    let tech_id = tech.json()["id"].as_i64().unwrap_or(0);
    let proj = json!({
        "slug":"test-proj","title":{"en":"My <b>proj</b><script>alert(1)</script>","it":"Mio"},"subtitle":{"en":"sub"},
        "description":{"en":"desc"},"hue":10,"status":"active","github_url":"https://github.com/x/y",
        "technology_ids":[tech_id],"visible":true,"started_on":"2026-01-01"
    });
    let created = call(&ctx, s.write(&ctx, Method::POST, "/api/admin/projects").json(proj.clone())).await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text());
    let pid = created.json()["id"].as_i64().unwrap_or(0);

    // riflesso nell'API pubblica (cache invalidata dal commit)
    let pubp = call(&ctx, Rq::new(Method::GET, "/data/projects.json", &ctx)).await.json();
    let item = pubp["projects"].as_array().and_then(|l| l.iter().find(|p| p["id"] == "test-proj")).cloned().expect("progetto pubblico");
    assert_eq!(item["technologies"], json!(["Rust"]));
    assert_eq!(item["github_url"], "https://github.com/x/y");
    let site = call(&ctx, Rq::new(Method::GET, "/data/site.json", &ctx)).await.json();
    assert!(site["projects"].as_array().map(|l| l.iter().any(|p| p["id"] == "test-proj")).unwrap_or(false));
    let lang = call(&ctx, Rq::new(Method::GET, "/lang/en.json", &ctx)).await.json();
    let title = lang["projectItems"]["test-proj"]["t"].as_str().unwrap_or("");
    assert!(title.contains("proj") && !title.contains("<script"), "XSS neutralizzato: {title}");

    // ETag / 304
    let first = call(&ctx, Rq::new(Method::GET, "/data/site.json", &ctx)).await;
    let etag = first.h("etag");
    assert!(!etag.is_empty());
    let mut req = Request::builder().uri("/data/site.json").header(header::IF_NONE_MATCH, &etag).body(Body::empty()).expect("req");
    req.extensions_mut().insert(ConnectInfo(SocketAddr::from(([10, 0, 0, 1], 1))));
    assert_eq!(ctx.router.clone().oneshot(req).await.expect("r").status(), StatusCode::NOT_MODIFIED);

    // validazione input
    for (label, patch) in [
        ("slug", json!({"slug":"Bad Slug!"})),
        ("hue", json!({"hue": 999})),
        ("stato", json!({"status":"boh"})),
        ("javascript:", json!({"demo_url":"javascript:alert(1)"})),
        ("tecnologia inesistente", json!({"technology_ids":[999999]})),
        ("date", json!({"started_on":"2026-05-01","ended_on":"2026-01-01"})),
    ] {
        let mut bad = proj.clone();
        bad["slug"] = json!(format!("v-{}", Uuid::new_v4().simple()));
        for (k, v) in patch.as_object().expect("obj") { bad[k] = v.clone(); }
        let r = call(&ctx, s.write(&ctx, Method::POST, "/api/admin/projects").json(bad)).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{label}: {}", r.text());
        assert!(r.json()["error"]["fields"].as_array().map(|f| !f.is_empty()).unwrap_or(false), "{label}");
    }
    let dup = call(&ctx, s.write(&ctx, Method::POST, "/api/admin/projects").json(proj.clone())).await;
    assert_eq!(dup.status, StatusCode::CONFLICT, "slug duplicato");
    let junk = call(&ctx, s.write(&ctx, Method::POST, "/api/admin/projects").raw("application/json", b"{non json".to_vec())).await;
    assert_eq!(junk.status, StatusCode::BAD_REQUEST);
    let huge = call(&ctx, s.write(&ctx, Method::POST, "/api/admin/projects").raw("application/json", vec![b' '; 1_200_000])).await;
    assert_eq!(huge.status, StatusCode::PAYLOAD_TOO_LARGE);
    let missing = call(&ctx, s.write(&ctx, Method::PUT, "/api/admin/projects/9999999").json(proj.clone())).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);

    // ordinamento: serve la lista completa
    let partial = call(&ctx, s.write(&ctx, Method::PUT, "/api/admin/projects/order").json(json!({"ids":[pid]}))).await;
    assert!(partial.status.is_client_error());
    let all: Vec<i64> = call(&ctx, s.get(&ctx, "/api/admin/projects")).await.json().as_array().expect("l").iter().filter_map(|p| p["id"].as_i64()).collect();
    let mut rev = all.clone();
    rev.reverse();
    assert!(call(&ctx, s.write(&ctx, Method::PUT, "/api/admin/projects/order").json(json!({"ids":rev}))).await.status.is_success());
    let site = call(&ctx, Rq::new(Method::GET, "/data/site.json", &ctx)).await.json();
    assert_eq!(site["projects"][0]["id"], "test-proj", "il nuovo ordine è pubblico");

    // visibilità
    let mut hidden = proj.clone();
    hidden["visible"] = json!(false);
    assert_eq!(call(&ctx, s.write(&ctx, Method::PUT, &format!("/api/admin/projects/{pid}")).json(hidden)).await.status, StatusCode::OK);
    let site = call(&ctx, Rq::new(Method::GET, "/data/site.json", &ctx)).await.json();
    assert!(!site["projects"].as_array().map(|l| l.iter().any(|p| p["id"] == "test-proj")).unwrap_or(true), "progetto nascosto non pubblico");

    // profilo, disponibilità e SEO
    let mut prof = call(&ctx, s.get(&ctx, "/api/admin/profile")).await.json();
    prof.as_object_mut().expect("o").remove("updated_at");
    prof["availability"] = json!("limited");
    prof["job_title"] = json!({"en":"Rustacean","it":"Rustacea"});
    assert_eq!(call(&ctx, s.write(&ctx, Method::PUT, "/api/admin/profile").json(prof)).await.status, StatusCode::OK);
    let site = call(&ctx, Rq::new(Method::GET, "/data/site.json", &ctx)).await.json();
    assert_eq!(site["availability"], "limited");
    assert_eq!(site["jobTitle"]["en"], "Rustacean");
    let mut seo = call(&ctx, s.get(&ctx, "/api/admin/settings/seo")).await.json();
    seo["site_title"] = json!({"en":"Titolo & co","it":"Titolo"});
    assert_eq!(call(&ctx, s.write(&ctx, Method::PUT, "/api/admin/settings/seo").json(seo)).await.status, StatusCode::OK);
    let index = call(&ctx, Rq::new(Method::GET, "/", &ctx)).await.text();
    assert!(index.contains("Titolo &amp; co"), "title escapato nell'HTML servito");
    let mut evil = call(&ctx, s.get(&ctx, "/api/admin/settings/seo")).await.json();
    evil["site_title"] = json!({"en":"<script>x</script>"});
    assert_eq!(call(&ctx, s.write(&ctx, Method::PUT, "/api/admin/settings/seo").json(evil)).await.status, StatusCode::UNPROCESSABLE_ENTITY, "markup nel titolo rifiutato");

    // cancellazione + audit
    assert_eq!(call(&ctx, s.write(&ctx, Method::DELETE, &format!("/api/admin/projects/{pid}"))).await.status, StatusCode::NO_CONTENT);
    let audit = call(&ctx, s.get(&ctx, "/api/admin/audit?action=content.&per_page=100")).await.json();
    let actions: Vec<&str> = audit["items"].as_array().expect("i").iter().filter_map(|a| a["action"].as_str()).collect();
    for want in ["content.project.create", "content.project.update", "content.project.delete", "content.technology.create", "content.profile.update", "content.project.reorder"] {
        assert!(actions.contains(&want), "manca {want} in {actions:?}");
    }
    let dash = call(&ctx, s.get(&ctx, "/api/admin/dashboard")).await.json();
    assert!(dash["stats"]["projects_total"].is_number() && dash["recent_changes"].is_array());
    ctx.finish().await;
}

#[tokio::test]
async fn media_upload_is_sniffed_limited_and_protected() {
    let ctx = setup(|_| {}).await;
    let s = admin_session(&ctx).await;

    let (ct, body) = multipart("../../evil name.png", &png());
    let up = call(&ctx, s.write(&ctx, Method::POST, "/api/admin/media").raw(&ct, body)).await;
    assert_eq!(up.status, StatusCode::CREATED, "{}", up.text());
    let m = up.json();
    let url = m["url"].as_str().unwrap_or("").to_string();
    assert!(url.starts_with("/uploads/") && url.ends_with(".png"));
    assert_eq!(m["original_name"], "evil name.png", "il nome originale è ripulito dal path");
    let file = call(&ctx, Rq::new(Method::GET, &url, &ctx)).await;
    assert_eq!(file.status, StatusCode::OK);
    assert_eq!(file.h("content-type"), "image/png");
    assert!(file.h("cache-control").contains("immutable"));
    assert_eq!(file.h("x-content-type-options"), "nosniff");

    // SVG/HTML camuffati da PNG rifiutati dal contenuto
    for bad in [&b"<svg xmlns='http://www.w3.org/2000/svg'><script>1</script></svg>"[..], b"<html><script>alert(1)</script>"] {
        let (ct, body) = multipart("a.png", bad);
        let r = call(&ctx, s.write(&ctx, Method::POST, "/api/admin/media").raw(&ct, body)).await;
        assert_eq!(r.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    // troppo grande
    let mut big = png();
    big.extend(vec![0u8; 1_300_000]);
    let (ct, body) = multipart("big.png", &big);
    assert_eq!(call(&ctx, s.write(&ctx, Method::POST, "/api/admin/media").raw(&ct, body)).await.status, StatusCode::PAYLOAD_TOO_LARGE);
    // senza CSRF
    let (ct, body) = multipart("a.png", &png());
    assert_eq!(call(&ctx, Rq::new(Method::POST, "/api/admin/media", &ctx).cookie(&s.cookie).raw(&ct, body)).await.status, StatusCode::FORBIDDEN);

    // path traversal / nomi non validi
    for p in ["/uploads/..%2f..%2fetc%2fpasswd", "/uploads/x.png", "/uploads/00000000-0000-0000-0000-000000000000.svg"] {
        assert_eq!(call(&ctx, Rq::new(Method::GET, p, &ctx)).await.status, StatusCode::NOT_FOUND, "{p}");
    }

    // un media in uso non si cancella
    let mid = m["id"].as_str().unwrap_or("").to_string();
    let proj = json!({"slug":"with-img","title":{"en":"T"},"hue":1,"status":"active","images":[{"media_id":mid,"alt":{"en":"alt"}}]});
    let created = call(&ctx, s.write(&ctx, Method::POST, "/api/admin/projects").json(proj)).await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text());
    let pubp = call(&ctx, Rq::new(Method::GET, "/data/projects.json", &ctx)).await.json();
    let imgs = pubp["projects"].as_array().and_then(|l| l.iter().find(|p| p["id"] == "with-img")).map(|p| p["images"].clone()).unwrap_or(Value::Null);
    assert_eq!(imgs[0]["url"], url);
    let del = call(&ctx, s.write(&ctx, Method::DELETE, &format!("/api/admin/media/{mid}"))).await;
    assert!(del.status.is_client_error(), "media in uso: {}", del.status);
    ctx.finish().await;
}

/* ───────────── superficie pubblica e header ───────────── */

#[tokio::test]
async fn public_surface_headers_and_static_file_rules() {
    let ctx = setup(|_| {}).await;

    let idx = call(&ctx, Rq::new(Method::GET, "/", &ctx)).await;
    assert_eq!(idx.status, StatusCode::OK);
    let csp = idx.h("content-security-policy");
    assert!(csp.contains("default-src 'self'") && csp.contains("frame-ancestors 'none'") && csp.contains("object-src 'none'"));
    assert!(!csp.contains("script-src 'self' 'unsafe-inline'"));
    assert_eq!(idx.h("x-frame-options"), "DENY");
    assert_eq!(idx.h("x-content-type-options"), "nosniff");
    assert!(!idx.h("referrer-policy").is_empty() && !idx.h("permissions-policy").is_empty());
    assert!(idx.h("access-control-allow-origin").is_empty(), "nessun CORS permissivo");
    assert!(idx.h("strict-transport-security").is_empty(), "HSTS solo su HTTPS");

    let adm = call(&ctx, Rq::new(Method::GET, "/admin/", &ctx)).await;
    assert_eq!(adm.status, StatusCode::OK);
    assert!(adm.h("content-security-policy").starts_with("default-src 'none'"));
    assert!(adm.h("content-type").starts_with("text/html"));
    assert!(adm.h("cache-control").contains("no-cache") || adm.h("cache-control").contains("no-store"));
    for p in ["/admin/admin.js", "/admin/admin.css"] {
        assert_eq!(call(&ctx, Rq::new(Method::GET, p, &ctx)).await.status, StatusCode::OK, "{p}");
    }
    assert!(adm.text().contains("/admin/admin.js") && !adm.text().contains("<script>"), "nessuno script inline nell'admin");

    let me = call(&ctx, Rq::new(Method::GET, "/api/me", &ctx)).await;
    assert_eq!(me.h("cache-control"), "no-store");
    assert!(me.h("content-security-policy").contains("sandbox"));

    for p in ["/script.js", "/style.css", "/index.html", "/robots.txt", "/sitemap.xml", "/healthz"] {
        assert_eq!(call(&ctx, Rq::new(Method::GET, p, &ctx)).await.status, StatusCode::OK, "{p}");
    }
    for p in [
        "/backend/Cargo.toml", "/backend/src/main.rs", "/.env", "/.git/config", "/../etc/passwd", "/%2e%2e/etc/passwd",
        "/migrations/0001_init.sql", "/data/other.json", "/lang/zz.json", "/admin/index.html", "/admin/..%2fCargo.toml", "/admin/.env",
        "/noextension",
    ] {
        let r = call(&ctx, Rq::new(Method::GET, p, &ctx)).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{p} → {}", r.status);
    }
    // il database non è mai esposto: gli errori non rivelano dettagli interni
    let bad = call(&ctx, Rq::new(Method::GET, "/api/admin/projects/abc", &ctx)).await;
    assert!(!bad.text().to_lowercase().contains("postgres") && !bad.text().contains("sqlx"));
    ctx.finish().await;
}

#[tokio::test]
async fn rate_limit_is_per_ip_and_reports_retry_after() {
    let ctx = setup(|c| c.rate_public_per_min = 3).await;
    for _ in 0..3 {
        assert_eq!(call(&ctx, Rq::new(Method::GET, "/healthz", &ctx).ip("10.1.1.1")).await.status, StatusCode::OK);
    }
    let blocked = call(&ctx, Rq::new(Method::GET, "/healthz", &ctx).ip("10.1.1.1")).await;
    assert_eq!(blocked.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(blocked.h("retry-after").parse::<u64>().unwrap_or(0) > 0);
    assert_eq!(call(&ctx, Rq::new(Method::GET, "/healthz", &ctx).ip("10.2.2.2")).await.status, StatusCode::OK, "un altro IP non è limitato");
    ctx.finish().await;
}

#[tokio::test]
async fn rate_limit_is_also_applied_per_user() {
    let ctx = setup(|c| c.rate_admin_per_min = 6).await;
    let s = admin_session(&ctx).await;
    let mut codes = Vec::new();
    for i in 0..12 {
        // IP diversi: il limite per utente deve scattare comunque
        let r = call(&ctx, s.get(&ctx, "/api/admin/dashboard").ip(&format!("10.9.0.{}", i + 1))).await;
        codes.push(r.status);
    }
    assert!(codes.contains(&StatusCode::TOO_MANY_REQUESTS), "{codes:?}");
    ctx.finish().await;
}

#[tokio::test]
async fn ui_texts_and_general_settings_roundtrip() {
    let ctx = setup(|_| {}).await;
    let s = admin_session(&ctx).await;
    let g = call(&ctx, s.get(&ctx, "/api/admin/settings/general")).await.json();
    assert_eq!(g["languages"], json!(["en", "it"]));

    // testi UI: modificabili, ma non quelli derivati da altre sezioni
    let ok = call(&ctx, s.write(&ctx, Method::PUT, "/api/admin/settings/ui-texts").json(json!({"lang":"en","items":[{"key":"nav.work","value":"projects"}]}))).await;
    assert_eq!(ok.status, StatusCode::NO_CONTENT);
    let lang = call(&ctx, Rq::new(Method::GET, "/lang/en.json", &ctx)).await.json();
    assert_eq!(lang["nav"]["work"], "projects");
    let derived = call(&ctx, s.write(&ctx, Method::PUT, "/api/admin/settings/ui-texts").json(json!({"lang":"en","items":[{"key":"hero.text","value":"x"}]}))).await;
    assert_eq!(derived.status, StatusCode::UNPROCESSABLE_ENTITY);
    let inactive = call(&ctx, s.write(&ctx, Method::PUT, "/api/admin/settings/ui-texts").json(json!({"lang":"fr","items":[{"key":"nav.work","value":"x"}]}))).await;
    assert_eq!(inactive.status, StatusCode::UNPROCESSABLE_ENTITY);
    let xss = call(&ctx, s.write(&ctx, Method::PUT, "/api/admin/settings/ui-texts").json(json!({"lang":"en","items":[{"key":"nav.about","value":"<img src=x onerror=alert(1)>"}]}))).await;
    assert_eq!(xss.status, StatusCode::NO_CONTENT);
    let lang = call(&ctx, Rq::new(Method::GET, "/lang/en.json", &ctx)).await.json();
    assert!(!lang["nav"]["about"].as_str().unwrap_or("").contains("<img"), "HTML pericoloso sanificato");

    // nuova lingua + copia delle voci
    let mut g2 = g.clone();
    g2["languages"] = json!(["en", "it", "fr"]);
    assert_eq!(call(&ctx, s.write(&ctx, Method::PUT, "/api/admin/settings/general").json(g2)).await.status, StatusCode::OK);
    let copied = call(&ctx, s.write(&ctx, Method::POST, "/api/admin/settings/ui-texts/copy").json(json!({"from":"en","to":"fr"}))).await;
    assert!(copied.json()["copied"].as_i64().unwrap_or(0) > 0);
    assert_eq!(call(&ctx, Rq::new(Method::GET, "/lang/fr.json", &ctx)).await.status, StatusCode::OK);

    // impostazioni non valide
    let mut bad = g.clone();
    bad["default_lang"] = json!("de");
    assert_eq!(call(&ctx, s.write(&ctx, Method::PUT, "/api/admin/settings/general").json(bad)).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    ctx.finish().await;
}
