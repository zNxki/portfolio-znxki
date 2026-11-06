use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use chrono::{Datelike, NaiveDate};
use serde_json::{json, Map, Value};
use sqlx::PgPool;
use tokio::sync::RwLock;
use uuid::Uuid;

use super::{contacts, experiences, profile, projects, settings, technologies};
use crate::error::{AppError, AppResult};
use crate::models::{GeneralSettings, Project, SeoSettings};
use crate::security::tokens::sha256_hex;

const CACHE_TTL: Duration = Duration::from_secs(60);

pub struct Payload {
    pub body: Bytes,
    pub etag: String,
}

pub struct Snapshot {
    pub site: Payload,
    pub langs: HashMap<String, Payload>,
    pub general: GeneralSettings,
    pub seo: SeoSettings,
    pub og_image: Option<String>,
}

#[derive(Default)]
pub struct SiteCache {
    inner: RwLock<Option<(Instant, Arc<Snapshot>)>>,
}

impl SiteCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn get(&self, pool: &PgPool) -> AppResult<Arc<Snapshot>> {
        {
            let guard = self.inner.read().await;
            if let Some((at, snap)) = guard.as_ref() {
                if at.elapsed() < CACHE_TTL {
                    return Ok(snap.clone());
                }
            }
        }
        let fresh = Arc::new(build(pool).await?);
        let mut guard = self.inner.write().await;
        *guard = Some((Instant::now(), fresh.clone()));
        Ok(fresh)
    }

    pub async fn invalidate(&self) {
        let mut guard = self.inner.write().await;
        *guard = None;
    }
}

pub fn set_path(map: &mut Map<String, Value>, path: &[&str], val: Value) {
    match path {
        [] => {}
        [last] => {
            map.insert((*last).to_string(), val);
        }
        [first, rest @ ..] => {
            let entry = map
                .entry((*first).to_string())
                .or_insert_with(|| Value::Object(Map::new()));
            if !entry.is_object() {
                *entry = Value::Object(Map::new());
            }
            if let Value::Object(inner) = entry {
                set_path(inner, rest, val);
            }
        }
    }
}

fn arrayify(v: Value) -> Value {
    match v {
        Value::Object(m) => {
            let n = m.len();
            let is_array = n > 0 && (0..n).all(|i| m.contains_key(&i.to_string()));
            if is_array {
                let mut items = Vec::with_capacity(n);
                for i in 0..n {
                    if let Some(x) = m.get(&i.to_string()) {
                        items.push(arrayify(x.clone()));
                    }
                }
                Value::Array(items)
            } else {
                Value::Object(m.into_iter().map(|(k, x)| (k, arrayify(x))).collect())
            }
        }
        other => other,
    }
}

pub fn unflatten(items: &[(String, String)]) -> Map<String, Value> {
    let mut root = Map::new();
    for (key, value) in items {
        let parts: Vec<&str> = key.split('.').collect();
        set_path(&mut root, &parts, Value::String(value.clone()));
    }
    match arrayify(Value::Object(root)) {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

pub fn flatten(value: &Value) -> Vec<(String, String)> {
    fn walk(prefix: &str, v: &Value, out: &mut Vec<(String, String)>) {
        match v {
            Value::String(s) => out.push((prefix.to_string(), s.clone())),
            Value::Object(m) => {
                for (k, x) in m {
                    let next = if prefix.is_empty() {
                        k.clone()
                    } else {
                        format!("{prefix}.{k}")
                    };
                    walk(&next, x, out);
                }
            }
            Value::Array(a) => {
                for (i, x) in a.iter().enumerate() {
                    let next = if prefix.is_empty() {
                        i.to_string()
                    } else {
                        format!("{prefix}.{i}")
                    };
                    walk(&next, x, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk("", value, &mut out);
    out
}

pub fn is_derived_key(key: &str) -> bool {
    key == "tag"
        || key == "meta.title"
        || key == "meta.description"
        || key == "hero.text"
        || key == "about.lead"
        || key.starts_with("about.p.")
        || key.starts_with("expItems.")
        || key.starts_with("projectItems.")
        || key.starts_with("skills.")
}

fn month_year(d: NaiveDate) -> String {
    format!("{:02}/{}", d.month(), d.year())
}

pub fn project_year(p: &Project) -> String {
    if let Some(label) = &p.year_label {
        if !label.trim().is_empty() {
            return label.clone();
        }
    }
    match (p.started_on, p.ended_on) {
        (Some(s), Some(e)) => {
            let a = month_year(s);
            let b = month_year(e);
            if a == b {
                a
            } else {
                format!("{a} - {b}")
            }
        }
        (Some(s), None) => {
            if p.status == "active" {
                format!("{} -", month_year(s))
            } else {
                month_year(s)
            }
        }
        _ => String::new(),
    }
}

fn payload(value: &Value) -> AppResult<Payload> {
    let body = serde_json::to_vec(value).map_err(|e| AppError::internal(e.to_string()))?;
    let hash = sha256_hex(&body);
    Ok(Payload {
        etag: format!("\"{}\"", &hash[..32]),
        body: Bytes::from(body),
    })
}

pub async fn build(pool: &PgPool) -> AppResult<Snapshot> {
    let general = settings::get_general(pool).await?;
    let seo = settings::get_seo(pool).await?;
    let prof = profile::get(pool).await?;
    let contact_rows: Vec<_> = contacts::list(pool)
        .await?
        .into_iter()
        .filter(|c| c.visible)
        .collect();
    let techs: Vec<_> = technologies::list(pool)
        .await?
        .into_iter()
        .filter(|t| t.visible)
        .collect();
    let exps: Vec<_> = experiences::list(pool)
        .await?
        .into_iter()
        .filter(|e| e.visible)
        .collect();
    let projs: Vec<_> = projects::list_plain(pool)
        .await?
        .into_iter()
        .filter(|p| p.visible)
        .collect();
    let mut ui = settings::ui_texts_all(pool).await?;

    let project_values: Vec<Value> = projs
        .iter()
        .map(|p| {
            let mut m = Map::new();
            m.insert("id".into(), json!(p.slug));
            if let Some(name) = &p.name {
                if !name.trim().is_empty() {
                    m.insert("name".into(), json!(name));
                }
            }
            m.insert("year".into(), json!(project_year(p)));
            m.insert("hue".into(), json!(p.hue));
            Value::Object(m)
        })
        .collect();

    let site = json!({
        "defaultLang": general.default_lang,
        "languages": general.languages,
        "name": { "full": prof.display_name, "parts": prof.name_parts.0 },
        "boot": serde_json::to_value(&general.boot).map_err(|e| AppError::internal(e.to_string()))?,
        "contacts": contact_rows.iter().map(|c| json!({
            "icon": c.icon, "label": c.label, "value": c.value, "href": c.href
        })).collect::<Vec<_>>(),
        "experience": exps.iter().map(|e| json!(e.slug)).collect::<Vec<_>>(),
        "projects": project_values,
        "marquee": techs.iter().filter(|t| t.in_marquee).map(|t| {
            json!(t.marquee_label.clone().filter(|l| !l.trim().is_empty()).unwrap_or_else(|| t.name.clone()))
        }).collect::<Vec<_>>(),
        "skills": techs.iter().map(|t| json!({ "name": t.name, "level": t.level })).collect::<Vec<_>>(),
        "levels": serde_json::to_value(&general.levels).map_err(|e| AppError::internal(e.to_string()))?,
        "accents": serde_json::to_value(&general.accents).map_err(|e| AppError::internal(e.to_string()))?,
        "sections": general.sections,
        "availability": prof.availability,
        "jobTitle": prof.job_title.0,
    });

    let mut langs = HashMap::new();
    for lang in &general.languages {
        let l = lang.as_str();
        let d = general.default_lang.as_str();
        let items = ui.remove(l).unwrap_or_default();
        let mut root = unflatten(&items);

        set_path(
            &mut root,
            &["meta", "title"],
            json!(seo
                .site_title
                .get(l)
                .or_else(|| seo.site_title.get(d))
                .cloned()
                .unwrap_or_default()),
        );
        set_path(
            &mut root,
            &["meta", "description"],
            json!(seo
                .site_description
                .get(l)
                .or_else(|| seo.site_description.get(d))
                .cloned()
                .unwrap_or_default()),
        );
        set_path(
            &mut root,
            &["tag"],
            json!(prof.availability_text.0.pick(l, d)),
        );
        set_path(
            &mut root,
            &["hero", "text"],
            json!(prof.headline.0.pick(l, d)),
        );
        set_path(
            &mut root,
            &["about", "lead"],
            json!(prof.bio_lead.0.pick(l, d)),
        );
        set_path(
            &mut root,
            &["about", "p"],
            Value::Array(
                prof.bio_paragraphs
                    .0
                    .pick(l, d)
                    .into_iter()
                    .map(Value::String)
                    .collect(),
            ),
        );

        let mut exp_items = Map::new();
        for e in &exps {
            exp_items.insert(
                e.slug.clone(),
                json!({ "t": e.title.0.pick(l, d), "s": e.subtitle.0.pick(l, d) }),
            );
        }
        root.insert("expItems".into(), Value::Object(exp_items));

        let mut project_items = Map::new();
        for p in &projs {
            project_items.insert(
                p.slug.clone(),
                json!({ "t": p.title.0.pick(l, d), "s": p.subtitle.0.pick(l, d) }),
            );
        }
        root.insert("projectItems".into(), Value::Object(project_items));

        let mut skill_items = Map::new();
        for t in &techs {
            skill_items.insert(t.name.clone(), json!(t.description.0.pick(l, d)));
        }
        root.insert("skills".into(), Value::Object(skill_items));

        langs.insert(lang.clone(), payload(&Value::Object(root))?);
    }

    let og_image = match seo.og_image_media {
        Some(id) => media_filename(pool, id).await?,
        None => None,
    };

    Ok(Snapshot {
        site: payload(&site)?,
        langs,
        general,
        seo,
        og_image,
    })
}

async fn media_filename(pool: &PgPool, id: Uuid) -> AppResult<Option<String>> {
    let name: Option<String> = sqlx::query_scalar("SELECT filename FROM media WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unflatten_builds_objects_and_arrays() {
        let items = vec![
            ("boot.lines.0".to_string(), "a".to_string()),
            ("boot.lines.1".to_string(), "".to_string()),
            ("boot.h1".to_string(), "x".to_string()),
            ("locale".to_string(), "it-IT".to_string()),
        ];
        let m = unflatten(&items);
        assert_eq!(m["locale"], json!("it-IT"));
        assert_eq!(m["boot"]["h1"], json!("x"));
        assert_eq!(m["boot"]["lines"], json!(["a", ""]));
    }

    #[test]
    fn flatten_roundtrips_with_unflatten() {
        let original = json!({
            "locale": "it-IT",
            "boot": { "h1": "x", "lines": ["a", "", "c"] },
            "console": { "help": "h" }
        });
        let flat = flatten(&original);
        assert_eq!(flat.len(), 6);
        let back = unflatten(&flat);
        assert_eq!(Value::Object(back), original);
    }

    #[test]
    fn derived_keys_are_excluded_from_ui_texts() {
        assert!(is_derived_key("tag"));
        assert!(is_derived_key("meta.title"));
        assert!(is_derived_key("about.p.0"));
        assert!(is_derived_key("skills.Node.js"));
        assert!(is_derived_key("expItems.js.t"));
        assert!(!is_derived_key("meta.away"));
        assert!(!is_derived_key("hero.label"));
        assert!(!is_derived_key("levels.advanced"));
    }

    #[test]
    fn set_path_replaces_non_objects() {
        let mut m = Map::new();
        m.insert("a".into(), json!("leaf"));
        set_path(&mut m, &["a", "b"], json!(1));
        assert_eq!(m["a"]["b"], json!(1));
    }

    fn proj(
        label: Option<&str>,
        s: Option<NaiveDate>,
        e: Option<NaiveDate>,
        status: &str,
    ) -> Project {
        use crate::models::I18n;
        use sqlx::types::Json;
        Project {
            id: 1,
            slug: "x".into(),
            name: None,
            title: Json(I18n::default()),
            subtitle: Json(I18n::default()),
            description: Json(I18n::default()),
            year_label: label.map(|s| s.to_string()),
            hue: 1,
            status: status.into(),
            github_url: None,
            demo_url: None,
            started_on: s,
            ended_on: e,
            visible: true,
            position: 0,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn project_year_formats() {
        let d = |y, m| NaiveDate::from_ymd_opt(y, m, 1);
        assert_eq!(
            project_year(&proj(Some("10/2026 -"), None, None, "active")),
            "10/2026 -"
        );
        assert_eq!(
            project_year(&proj(None, d(2026, 10), None, "active")),
            "10/2026 -"
        );
        assert_eq!(
            project_year(&proj(None, d(2026, 10), None, "completed")),
            "10/2026"
        );
        assert_eq!(
            project_year(&proj(None, d(2025, 1), d(2026, 3), "completed")),
            "01/2025 - 03/2026"
        );
        assert_eq!(
            project_year(&proj(None, d(2026, 11), d(2026, 11), "completed")),
            "11/2026"
        );
        assert_eq!(project_year(&proj(None, None, None, "active")), "");
    }
}
