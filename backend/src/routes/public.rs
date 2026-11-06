use std::collections::BTreeMap;
use std::path::{Path as FsPath, PathBuf};

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::database::site::Payload;
use crate::database::{projects, technologies};
use crate::error::{AppError, AppResult};
use crate::models::SeoSettings;
use crate::security::validation::html_escape;
use crate::state::AppState;

const STATIC_EXTENSIONS: [&str; 11] = [
    "css", "js", "ico", "png", "svg", "webp", "jpg", "jpeg", "gif", "woff2", "txt",
];
const ADMIN_EXTENSIONS: [&str; 6] = ["css", "js", "svg", "png", "ico", "woff2"];
const DENIED_ROOTS: [&str; 12] = [
    "data",
    "lang",
    "backend",
    "admin",
    "uploads",
    "api",
    "auth",
    "migrations",
    "target",
    "node_modules",
    "deploy",
    "src",
];

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(index))
        .route("/index.html", get(index))
        .route("/data/site.json", get(site_json))
        .route("/data/projects.json", get(projects_json))
        .route("/lang/{file}", get(lang_file))
        .route("/robots.txt", get(robots))
        .route("/sitemap.xml", get(sitemap))
        .route("/healthz", get(health))
        .route("/uploads/{name}", get(upload))
        .route("/admin", get(admin_redirect))
        .route("/admin/", get(admin_index))
        .route("/admin/{file}", get(admin_asset))
}

pub fn mime_for(ext: &str) -> &'static str {
    match ext {
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn extension_of(name: &str) -> Option<String> {
    name.rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .filter(|e| !e.is_empty())
}

fn remove_between(html: &str, start: &str, end: &str) -> String {
    if let Some(s) = html.find(start) {
        if let Some(e) = html[s..].find(end) {
            let mut out = String::with_capacity(html.len());
            out.push_str(&html[..s]);
            out.push_str(&html[s + e + end.len()..]);
            return out;
        }
    }
    html.to_string()
}

fn pick(map: &BTreeMap<String, String>, lang: &str) -> String {
    map.get(lang)
        .filter(|s| !s.is_empty())
        .or_else(|| map.values().next())
        .cloned()
        .unwrap_or_default()
}

pub fn render_index(
    template: &str,
    origin: &str,
    default_lang: &str,
    seo: &SeoSettings,
    og_image: Option<&str>,
) -> String {
    let title = pick(&seo.site_title, default_lang);
    let description = pick(&seo.site_description, default_lang);
    let og_title = {
        let t = pick(&seo.og_title, default_lang);
        if t.is_empty() {
            title.clone()
        } else {
            t
        }
    };
    let og_description = {
        let d = pick(&seo.og_description, default_lang);
        if d.is_empty() {
            description.clone()
        } else {
            d
        }
    };
    let keywords = pick(&seo.keywords, default_lang);

    let mut block = String::new();
    block.push_str(&format!("<title>{}</title>\n", html_escape(&title)));
    block.push_str(&format!(
        "<meta name=\"description\" content=\"{}\">\n",
        html_escape(&description)
    ));
    if !keywords.is_empty() {
        block.push_str(&format!(
            "<meta name=\"keywords\" content=\"{}\">\n",
            html_escape(&keywords)
        ));
    }
    block.push_str(&format!(
        "<meta name=\"robots\" content=\"{}\">\n",
        if seo.robots_index {
            "index,follow"
        } else {
            "noindex,nofollow"
        }
    ));
    if let Some(color) = &seo.theme_color {
        block.push_str(&format!(
            "<meta name=\"theme-color\" content=\"{}\">\n",
            html_escape(color)
        ));
    }
    block.push_str(&format!(
        "<link rel=\"canonical\" href=\"{}/\">\n",
        html_escape(origin)
    ));
    block.push_str("<meta property=\"og:type\" content=\"website\">\n");
    block.push_str(&format!(
        "<meta property=\"og:title\" content=\"{}\">\n",
        html_escape(&og_title)
    ));
    block.push_str(&format!(
        "<meta property=\"og:description\" content=\"{}\">\n",
        html_escape(&og_description)
    ));
    block.push_str(&format!(
        "<meta property=\"og:url\" content=\"{}/\">\n",
        html_escape(origin)
    ));
    block.push_str(&format!(
        "<meta name=\"twitter:card\" content=\"{}\">\n",
        if og_image.is_some() {
            "summary_large_image"
        } else {
            "summary"
        }
    ));
    block.push_str(&format!(
        "<meta name=\"twitter:title\" content=\"{}\">\n",
        html_escape(&og_title)
    ));
    block.push_str(&format!(
        "<meta name=\"twitter:description\" content=\"{}\">\n",
        html_escape(&og_description)
    ));
    if let Some(file) = og_image {
        let url = format!("{origin}/uploads/{file}");
        block.push_str(&format!(
            "<meta property=\"og:image\" content=\"{}\">\n",
            html_escape(&url)
        ));
        block.push_str(&format!(
            "<meta name=\"twitter:image\" content=\"{}\">\n",
            html_escape(&url)
        ));
    }

    let stripped = remove_between(template, "<title", "</title>");
    let stripped = remove_between(&stripped, "<meta name=\"description\"", ">");
    stripped.replacen("</head>", &format!("{block}</head>"), 1)
}

fn etag_matches(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            v.split(',')
                .map(|t| t.trim().trim_start_matches("W/"))
                .any(|t| t == etag || t == "*")
        })
        .unwrap_or(false)
}

fn json_payload(headers: &HeaderMap, payload: &Payload) -> Response {
    if etag_matches(headers, &payload.etag) {
        return (
            StatusCode::NOT_MODIFIED,
            [
                (header::ETAG, payload.etag.as_str()),
                (header::CACHE_CONTROL, "no-cache"),
            ],
        )
            .into_response();
    }
    (
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::ETAG, payload.etag.as_str()),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        payload.body.clone(),
    )
        .into_response()
}

fn file_response(mime: &'static str, cache: &'static str, body: Vec<u8>) -> Response {
    (
        [(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, cache)],
        body,
    )
        .into_response()
}

async fn read_or_404(path: &FsPath) -> AppResult<Vec<u8>> {
    tokio::fs::read(path).await.map_err(|_| AppError::NotFound)
}

async fn index(State(st): State<AppState>) -> AppResult<Response> {
    let snap = st.cache.get(&st.pool).await?;
    let html = render_index(
        &st.index_template,
        &st.cfg.origin(),
        &snap.general.default_lang,
        &snap.seo,
        snap.og_image.as_deref(),
    );
    Ok((
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        html,
    )
        .into_response())
}

async fn site_json(State(st): State<AppState>, headers: HeaderMap) -> AppResult<Response> {
    let snap = st.cache.get(&st.pool).await?;
    Ok(json_payload(&headers, &snap.site))
}

async fn lang_file(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(file): Path<String>,
) -> AppResult<Response> {
    let code = file.strip_suffix(".json").ok_or(AppError::NotFound)?;
    let snap = st.cache.get(&st.pool).await?;
    let payload = snap.langs.get(code).ok_or(AppError::NotFound)?;
    Ok(json_payload(&headers, payload))
}

async fn projects_json(State(st): State<AppState>) -> AppResult<Response> {
    let list = projects::list_full(&st.pool, true).await?;
    let techs = technologies::list(&st.pool).await?;
    let items: Vec<Value> = list
        .iter()
        .map(|p| {
            let names: Vec<String> = p
                .technology_ids
                .iter()
                .filter_map(|id| techs.iter().find(|t| t.id == *id && t.visible))
                .map(|t| t.name.clone())
                .collect();
            let images: Vec<Value> = p
                .images
                .iter()
                .map(|i| json!({ "url": i.url, "alt": i.alt }))
                .collect();
            json!({
                "id": p.project.slug,
                "name": p.project.name,
                "title": p.project.title.0,
                "subtitle": p.project.subtitle.0,
                "description": p.project.description.0,
                "status": p.project.status,
                "github_url": p.project.github_url,
                "demo_url": p.project.demo_url,
                "started_on": p.project.started_on,
                "ended_on": p.project.ended_on,
                "technologies": names,
                "images": images
            })
        })
        .collect();
    Ok((
        [(header::CACHE_CONTROL, "public, max-age=60")],
        Json(json!({ "projects": items })),
    )
        .into_response())
}

async fn robots(State(st): State<AppState>) -> AppResult<Response> {
    let snap = st.cache.get(&st.pool).await?;
    let body = if snap.seo.robots_index {
        format!(
            "User-agent: *\nAllow: /\nDisallow: /admin\nDisallow: /api\nDisallow: /auth\nSitemap: {}/sitemap.xml\n",
            st.cfg.origin()
        )
    } else {
        "User-agent: *\nDisallow: /\n".to_string()
    };
    Ok(file_response(
        "text/plain; charset=utf-8",
        "public, max-age=300",
        body.into_bytes(),
    ))
}

async fn sitemap(State(st): State<AppState>) -> AppResult<Response> {
    let snap = st.cache.get(&st.pool).await?;
    if !snap.seo.robots_index {
        return Err(AppError::NotFound);
    }
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\"><url><loc>{}/</loc></url></urlset>\n",
        html_escape(&st.cfg.origin())
    );
    Ok(file_response(
        "application/xml; charset=utf-8",
        "public, max-age=300",
        body.into_bytes(),
    ))
}

async fn health(State(st): State<AppState>) -> Response {
    match sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&st.pool)
        .await
    {
        Ok(_) => Json(json!({ "status": "ok" })).into_response(),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "unavailable" })),
        )
            .into_response(),
    }
}

pub fn upload_mime(name: &str) -> Option<(&'static str, String)> {
    let (stem, ext) = name.split_once('.')?;
    let id = Uuid::parse_str(stem).ok()?;
    if id.hyphenated().to_string() != stem {
        return None;
    }
    let mime = match ext {
        "png" => "image/png",
        "jpg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => return None,
    };
    Some((mime, name.to_string()))
}

async fn upload(State(st): State<AppState>, Path(name): Path<String>) -> AppResult<Response> {
    let (mime, file) = upload_mime(&name).ok_or(AppError::NotFound)?;
    let body = read_or_404(&st.cfg.upload_dir.join(file)).await?;
    Ok(file_response(
        mime,
        "public, max-age=31536000, immutable",
        body,
    ))
}

async fn admin_redirect() -> Redirect {
    Redirect::permanent("/admin/")
}

async fn admin_index(State(st): State<AppState>) -> AppResult<Response> {
    let body = read_or_404(&st.cfg.admin_dir.join("index.html")).await?;
    Ok(file_response(
        "text/html; charset=utf-8",
        "no-cache",
        body,
    ))
}

fn plain_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 80
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

async fn admin_asset(State(st): State<AppState>, Path(file): Path<String>) -> AppResult<Response> {
    if !plain_file_name(&file) {
        return Err(AppError::NotFound);
    }
    let ext = extension_of(&file).ok_or(AppError::NotFound)?;
    if !ADMIN_EXTENSIONS.contains(&ext.as_str()) {
        return Err(AppError::NotFound);
    }
    let body = read_or_404(&st.cfg.admin_dir.join(&file)).await?;
    Ok(file_response(mime_for(&ext), "no-cache", body))
}

pub fn safe_relative(path: &str) -> Option<PathBuf> {
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() || trimmed.len() > 200 {
        return None;
    }
    let segments: Vec<&str> = trimmed.split('/').collect();
    let mut out = PathBuf::new();
    for (i, seg) in segments.iter().enumerate() {
        if seg.is_empty()
            || seg.starts_with('.')
            || seg.contains('\\')
            || seg.contains('\0')
            || seg.contains('%')
        {
            return None;
        }
        if i == 0 && segments.len() > 1 && DENIED_ROOTS.contains(seg) {
            return None;
        }
        out.push(seg);
    }
    let last = segments.last()?;
    let ext = extension_of(last)?;
    if !STATIC_EXTENSIONS.contains(&ext.as_str()) {
        return None;
    }
    Some(out)
}

pub async fn static_file(State(st): State<AppState>, uri: Uri) -> AppResult<Response> {
    let rel = safe_relative(uri.path()).ok_or(AppError::NotFound)?;
    let root = tokio::fs::canonicalize(&st.cfg.frontend_dir)
        .await
        .map_err(|_| AppError::NotFound)?;
    let full = tokio::fs::canonicalize(root.join(&rel))
        .await
        .map_err(|_| AppError::NotFound)?;
    if !full.starts_with(&root) {
        return Err(AppError::NotFound);
    }
    let ext = full
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .ok_or(AppError::NotFound)?;
    let body = read_or_404(&full).await?;
    Ok(file_response(mime_for(&ext), "public, max-age=300", body))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seo() -> SeoSettings {
        let mut title = BTreeMap::new();
        title.insert("it".to_string(), "Titolo <x> & co".to_string());
        let mut desc = BTreeMap::new();
        desc.insert("it".to_string(), "Descrizione \"quotata\"".to_string());
        SeoSettings {
            site_title: title,
            site_description: desc,
            og_title: BTreeMap::new(),
            og_description: BTreeMap::new(),
            keywords: BTreeMap::new(),
            og_image_media: None,
            theme_color: Some("#0a0a0b".to_string()),
            robots_index: true,
        }
    }

    const TEMPLATE: &str = "<html><head><meta charset=\"utf-8\"><meta name=\"description\" content=\"\"><title>old</title><script>let a=1;</script></head><body></body></html>";

    #[test]
    fn render_replaces_title_and_escapes() {
        let out = render_index(TEMPLATE, "https://x.test", "it", &seo(), Some("a.png"));
        assert!(out.contains("<title>Titolo &lt;x&gt; &amp; co</title>"));
        assert!(!out.contains("<title>old</title>"));
        assert!(out.contains("content=\"Descrizione &quot;quotata&quot;\""));
        assert!(out.contains("og:image\" content=\"https://x.test/uploads/a.png\""));
        assert!(out.contains("<script>let a=1;</script>"));
        assert_eq!(out.matches("name=\"description\"").count(), 1);
        assert!(out.contains("noindex") == false);
    }

    #[test]
    fn render_respects_noindex() {
        let mut s = seo();
        s.robots_index = false;
        let out = render_index(TEMPLATE, "https://x.test", "it", &s, None);
        assert!(out.contains("noindex,nofollow"));
        assert!(out.contains("twitter:card\" content=\"summary\""));
    }

    #[test]
    fn static_paths_reject_traversal_and_private_roots() {
        assert!(safe_relative("/style.css").is_some());
        assert!(safe_relative("/img/logo.png").is_some());
        for bad in [
            "/",
            "/../etc/passwd",
            "/.env",
            "/a/.git/config",
            "/data/site.json",
            "/lang/it.json",
            "/backend/Cargo.toml",
            "/backend/src/main.rs",
            "/%2e%2e/x.css",
            "/index.html",
            "/script.js/",
            "/a\\b.css",
            "/noext",
        ] {
            assert!(safe_relative(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn upload_names_are_strict() {
        let id = Uuid::new_v4();
        assert!(upload_mime(&format!("{id}.png")).is_some());
        assert!(upload_mime(&format!("{id}.svg")).is_none());
        assert!(upload_mime("../x.png").is_none());
        assert!(upload_mime(&format!("{}.png", id.simple())).is_none());
        assert!(upload_mime(&format!("{id}.png.exe")).is_none());
    }

    #[test]
    fn etag_matching() {
        let mut h = HeaderMap::new();
        h.insert(header::IF_NONE_MATCH, "W/\"abc\", \"def\"".parse().unwrap_or_else(|_| header::HeaderValue::from_static("")));
        assert!(etag_matches(&h, "\"abc\""));
        assert!(etag_matches(&h, "\"def\""));
        assert!(!etag_matches(&h, "\"zzz\""));
    }

    #[test]
    fn admin_file_names() {
        assert!(plain_file_name("admin.js"));
        assert!(!plain_file_name(".hidden"));
        assert!(!plain_file_name("a/b.js"));
        assert!(!plain_file_name(""));
    }
}