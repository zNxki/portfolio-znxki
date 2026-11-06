use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use sqlx::FromRow;
use uuid::Uuid;

use crate::error::AppResult;
use crate::security::validation::{
    email, hex_color, href, https_url_opt, i18n_map, in_range, lang_code, plain, plain_opt,
    sanitize_inline_html, slug, Errors,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Role {
    Admin,
    User,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Admin => "ADMIN",
            Role::User => "USER",
        }
    }

    pub fn parse(value: &str) -> Option<Role> {
        match value {
            "ADMIN" => Some(Role::Admin),
            "USER" => Some(Role::User),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct I18n(pub BTreeMap<String, String>);

impl I18n {
    pub fn pick(&self, lang: &str, default_lang: &str) -> String {
        self.0
            .get(lang)
            .filter(|s| !s.is_empty())
            .or_else(|| self.0.get(default_lang))
            .or_else(|| self.0.values().next())
            .cloned()
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct I18nList(pub BTreeMap<String, Vec<String>>);

impl I18nList {
    pub fn pick(&self, lang: &str, default_lang: &str) -> Vec<String> {
        self.0
            .get(lang)
            .filter(|v| !v.is_empty())
            .or_else(|| self.0.get(default_lang))
            .or_else(|| self.0.values().next())
            .cloned()
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct User {
    pub id: Uuid,
    pub github_id: i64,
    pub login: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub role: String,
    pub created_at: DateTime<Utc>,
    pub last_login_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Profile {
    pub display_name: String,
    pub name_parts: Json<Vec<String>>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub job_title: Json<I18n>,
    pub headline: Json<I18n>,
    pub bio_lead: Json<I18n>,
    pub bio_paragraphs: Json<I18nList>,
    pub email: String,
    pub availability: String,
    pub availability_text: Json<I18n>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileInput {
    pub display_name: String,
    pub name_parts: Vec<String>,
    #[serde(default)]
    pub first_name: Option<String>,
    #[serde(default)]
    pub last_name: Option<String>,
    #[serde(default)]
    pub job_title: BTreeMap<String, String>,
    #[serde(default)]
    pub headline: BTreeMap<String, String>,
    #[serde(default)]
    pub bio_lead: BTreeMap<String, String>,
    #[serde(default)]
    pub bio_paragraphs: BTreeMap<String, Vec<String>>,
    pub email: String,
    pub availability: String,
    #[serde(default)]
    pub availability_text: BTreeMap<String, String>,
}

pub struct CleanProfile {
    pub display_name: String,
    pub name_parts: Vec<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub job_title: I18n,
    pub headline: I18n,
    pub bio_lead: I18n,
    pub bio_paragraphs: I18nList,
    pub email: String,
    pub availability: String,
    pub availability_text: I18n,
}

impl ProfileInput {
    pub fn validate(&self) -> AppResult<CleanProfile> {
        let mut e = Errors::new();
        let display_name = plain(&mut e, "display_name", &self.display_name, 1, 40);

        if self.name_parts.len() != 3 {
            e.push("name_parts", "Servono esattamente 3 parti del nome");
        }
        for part in &self.name_parts {
            plain(&mut e, "name_parts", part, 0, 40);
        }
        if self.name_parts.concat() != display_name {
            e.push("name_parts", "Le tre parti unite devono coincidere con il nome visualizzato");
        }

        let first_name = plain_opt(&mut e, "first_name", &self.first_name, 80);
        let last_name = plain_opt(&mut e, "last_name", &self.last_name, 80);
        let job_title = I18n(i18n_map(&mut e, "job_title", &self.job_title, false, 120, false));
        let headline = I18n(i18n_map(&mut e, "headline", &self.headline, false, 800, true));
        let bio_lead = I18n(i18n_map(&mut e, "bio_lead", &self.bio_lead, false, 500, true));

        let mut paragraphs = BTreeMap::new();
        for (lang, items) in &self.bio_paragraphs {
            lang_code(&mut e, "bio_paragraphs", lang);
            if items.len() > 12 {
                e.push("bio_paragraphs", "Troppi paragrafi (max 12)");
                continue;
            }
            let mut out = Vec::new();
            for item in items {
                let t = item.trim();
                if t.is_empty() {
                    continue;
                }
                if t.chars().count() > 1500 {
                    e.push("bio_paragraphs", "Paragrafo troppo lungo");
                    continue;
                }
                out.push(sanitize_inline_html(t));
            }
            paragraphs.insert(lang.trim().to_string(), out);
        }

        let mail = email(&mut e, "email", &self.email);
        if !matches!(self.availability.as_str(), "available" | "limited" | "unavailable") {
            e.push("availability", "Valore non valido");
        }
        let availability_text =
            I18n(i18n_map(&mut e, "availability_text", &self.availability_text, false, 200, true));

        e.finish()?;
        Ok(CleanProfile {
            display_name,
            name_parts: self.name_parts.clone(),
            first_name,
            last_name,
            job_title,
            headline,
            bio_lead,
            bio_paragraphs: I18nList(paragraphs),
            email: mail,
            availability: self.availability.clone(),
            availability_text,
        })
    }
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Contact {
    pub id: i64,
    pub icon: String,
    pub label: String,
    pub value: String,
    pub href: String,
    pub position: i32,
    pub visible: bool,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContactInput {
    pub icon: String,
    pub label: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub href: String,
    #[serde(default = "default_true")]
    pub visible: bool,
}

fn default_true() -> bool {
    true
}

pub struct CleanContact {
    pub icon: String,
    pub label: String,
    pub value: String,
    pub href: String,
    pub visible: bool,
}

impl ContactInput {
    pub fn validate(&self, profile_email: &str) -> AppResult<CleanContact> {
        let mut e = Errors::new();
        if !matches!(self.icon.as_str(), "email" | "telegram" | "github" | "discord") {
            e.push("icon", "Icona non supportata dal frontend");
        }
        let label = plain(&mut e, "label", &self.label, 1, 40);
        let (value, link, visible) = if self.icon == "email" {
            (profile_email.to_string(), format!("mailto:{profile_email}"), true)
        } else {
            let v = plain(&mut e, "value", &self.value, 1, 120);
            let h = href(&mut e, "href", &self.href);
            (v, h, self.visible)
        };
        e.finish()?;
        Ok(CleanContact {
            icon: self.icon.clone(),
            label,
            value,
            href: link,
            visible,
        })
    }
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Technology {
    pub id: i64,
    pub name: String,
    pub marquee_label: Option<String>,
    pub icon: Option<String>,
    pub level: i32,
    pub description: Json<I18n>,
    pub in_marquee: bool,
    pub visible: bool,
    pub position: i32,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TechnologyInput {
    pub name: String,
    #[serde(default)]
    pub marquee_label: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    pub level: i32,
    #[serde(default)]
    pub description: BTreeMap<String, String>,
    #[serde(default = "default_true")]
    pub in_marquee: bool,
    #[serde(default = "default_true")]
    pub visible: bool,
}

pub struct CleanTechnology {
    pub name: String,
    pub marquee_label: Option<String>,
    pub icon: Option<String>,
    pub level: i32,
    pub description: I18n,
    pub in_marquee: bool,
    pub visible: bool,
}

impl TechnologyInput {
    pub fn validate(&self) -> AppResult<CleanTechnology> {
        let mut e = Errors::new();
        let name = plain(&mut e, "name", &self.name, 1, 40);
        let marquee_label = plain_opt(&mut e, "marquee_label", &self.marquee_label, 40);
        let icon = plain_opt(&mut e, "icon", &self.icon, 40);
        in_range(&mut e, "level", self.level as i64, 0, 100);
        let description = I18n(i18n_map(&mut e, "description", &self.description, false, 300, false));
        e.finish()?;
        Ok(CleanTechnology {
            name,
            marquee_label,
            icon,
            level: self.level,
            description,
            in_marquee: self.in_marquee,
            visible: self.visible,
        })
    }
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Experience {
    pub id: i64,
    pub slug: String,
    pub kind: String,
    pub title: Json<I18n>,
    pub subtitle: Json<I18n>,
    pub role: Json<I18n>,
    pub organization: Option<String>,
    pub description: Json<I18n>,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    pub visible: bool,
    pub position: i32,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceInput {
    pub slug: String,
    pub kind: String,
    pub title: BTreeMap<String, String>,
    #[serde(default)]
    pub subtitle: BTreeMap<String, String>,
    #[serde(default)]
    pub role: BTreeMap<String, String>,
    #[serde(default)]
    pub organization: Option<String>,
    #[serde(default)]
    pub description: BTreeMap<String, String>,
    #[serde(default)]
    pub start_date: Option<NaiveDate>,
    #[serde(default)]
    pub end_date: Option<NaiveDate>,
    #[serde(default = "default_true")]
    pub visible: bool,
}

pub struct CleanExperience {
    pub slug: String,
    pub kind: String,
    pub title: I18n,
    pub subtitle: I18n,
    pub role: I18n,
    pub organization: Option<String>,
    pub description: I18n,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    pub visible: bool,
}

impl ExperienceInput {
    pub fn validate(&self) -> AppResult<CleanExperience> {
        let mut e = Errors::new();
        let s = slug(&mut e, "slug", &self.slug);
        if !matches!(self.kind.as_str(), "work" | "personal") {
            e.push("kind", "Tipo non valido");
        }
        let title = I18n(i18n_map(&mut e, "title", &self.title, true, 120, true));
        let subtitle = I18n(i18n_map(&mut e, "subtitle", &self.subtitle, false, 200, true));
        let role = I18n(i18n_map(&mut e, "role", &self.role, false, 120, false));
        let organization = plain_opt(&mut e, "organization", &self.organization, 120);
        let description = I18n(i18n_map(&mut e, "description", &self.description, false, 2000, true));
        if let (Some(a), Some(b)) = (self.start_date, self.end_date) {
            if b < a {
                e.push("end_date", "La data di fine precede quella di inizio");
            }
        }
        e.finish()?;
        Ok(CleanExperience {
            slug: s,
            kind: self.kind.clone(),
            title,
            subtitle,
            role,
            organization,
            description,
            start_date: self.start_date,
            end_date: self.end_date,
            visible: self.visible,
        })
    }
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Media {
    pub id: Uuid,
    pub filename: String,
    pub original_name: Option<String>,
    pub mime: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MediaOut {
    #[serde(flatten)]
    pub media: Media,
    pub url: String,
}

impl From<Media> for MediaOut {
    fn from(media: Media) -> Self {
        let url = format!("/uploads/{}", media.filename);
        MediaOut { media, url }
    }
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Project {
    pub id: i64,
    pub slug: String,
    pub name: Option<String>,
    pub title: Json<I18n>,
    pub subtitle: Json<I18n>,
    pub description: Json<I18n>,
    pub year_label: Option<String>,
    pub hue: i32,
    pub status: String,
    pub github_url: Option<String>,
    pub demo_url: Option<String>,
    pub started_on: Option<NaiveDate>,
    pub ended_on: Option<NaiveDate>,
    pub visible: bool,
    pub position: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ProjectImage {
    pub id: i64,
    pub project_id: i64,
    pub media_id: Uuid,
    pub alt: Json<I18n>,
    pub position: i32,
    pub filename: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectImageOut {
    pub id: i64,
    pub media_id: Uuid,
    pub alt: I18n,
    pub position: i32,
    pub url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectFull {
    #[serde(flatten)]
    pub project: Project,
    pub technology_ids: Vec<i64>,
    pub images: Vec<ProjectImageOut>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectImageInput {
    pub media_id: Uuid,
    #[serde(default)]
    pub alt: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectInput {
    pub slug: String,
    #[serde(default)]
    pub name: Option<String>,
    pub title: BTreeMap<String, String>,
    #[serde(default)]
    pub subtitle: BTreeMap<String, String>,
    #[serde(default)]
    pub description: BTreeMap<String, String>,
    #[serde(default)]
    pub year_label: Option<String>,
    pub hue: i32,
    pub status: String,
    #[serde(default)]
    pub github_url: Option<String>,
    #[serde(default)]
    pub demo_url: Option<String>,
    #[serde(default)]
    pub started_on: Option<NaiveDate>,
    #[serde(default)]
    pub ended_on: Option<NaiveDate>,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub technology_ids: Vec<i64>,
    #[serde(default)]
    pub images: Vec<ProjectImageInput>,
}

pub struct CleanProject {
    pub slug: String,
    pub name: Option<String>,
    pub title: I18n,
    pub subtitle: I18n,
    pub description: I18n,
    pub year_label: Option<String>,
    pub hue: i32,
    pub status: String,
    pub github_url: Option<String>,
    pub demo_url: Option<String>,
    pub started_on: Option<NaiveDate>,
    pub ended_on: Option<NaiveDate>,
    pub visible: bool,
    pub technology_ids: Vec<i64>,
    pub images: Vec<(Uuid, I18n)>,
}

impl ProjectInput {
    pub fn validate(&self) -> AppResult<CleanProject> {
        let mut e = Errors::new();
        let s = slug(&mut e, "slug", &self.slug);
        let name = plain_opt(&mut e, "name", &self.name, 80);
        let title = I18n(i18n_map(&mut e, "title", &self.title, true, 120, true));
        let subtitle = I18n(i18n_map(&mut e, "subtitle", &self.subtitle, false, 200, true));
        let description = I18n(i18n_map(&mut e, "description", &self.description, false, 4000, true));
        let year_label = plain_opt(&mut e, "year_label", &self.year_label, 30);
        in_range(&mut e, "hue", self.hue as i64, 0, 359);
        if !matches!(self.status.as_str(), "active" | "completed" | "paused" | "archived") {
            e.push("status", "Stato non valido");
        }
        let github_url = https_url_opt(&mut e, "github_url", &self.github_url);
        let demo_url = https_url_opt(&mut e, "demo_url", &self.demo_url);
        if let (Some(a), Some(b)) = (self.started_on, self.ended_on) {
            if b < a {
                e.push("ended_on", "La data di fine precede quella di inizio");
            }
        }
        if self.technology_ids.len() > 100 {
            e.push("technology_ids", "Troppe tecnologie");
        }
        let technology_ids: Vec<i64> = self
            .technology_ids
            .iter()
            .copied()
            .collect::<BTreeSet<i64>>()
            .into_iter()
            .collect();
        if self.images.len() > 30 {
            e.push("images", "Troppe immagini (max 30)");
        }
        let mut images = Vec::new();
        for img in &self.images {
            let alt = I18n(i18n_map(&mut e, "images", &img.alt, false, 200, false));
            images.push((img.media_id, alt));
        }
        e.finish()?;
        Ok(CleanProject {
            slug: s,
            name,
            title,
            subtitle,
            description,
            year_label,
            hue: self.hue,
            status: self.status.clone(),
            github_url,
            demo_url,
            started_on: self.started_on,
            ended_on: self.ended_on,
            visible: self.visible,
            technology_ids,
            images,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrderInput {
    pub ids: Vec<i64>,
}

impl OrderInput {
    pub fn validate(&self) -> AppResult<Vec<i64>> {
        let mut e = Errors::new();
        if self.ids.is_empty() || self.ids.len() > 500 {
            e.push("ids", "Elenco non valido");
        }
        let unique: BTreeSet<&i64> = self.ids.iter().collect();
        if unique.len() != self.ids.len() {
            e.push("ids", "Elenco con duplicati");
        }
        e.finish()?;
        Ok(self.ids.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BootSettings {
    pub version: String,
    pub build: String,
    pub kernel: String,
    pub cpu: u32,
    pub ram: u32,
    pub user: String,
    pub command: String,
    pub kinds: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LevelRule {
    pub min: u32,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Accents {
    pub dark: Vec<String>,
    pub light: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GeneralSettings {
    pub default_lang: String,
    pub languages: Vec<String>,
    pub boot: BootSettings,
    pub levels: Vec<LevelRule>,
    pub accents: Accents,
    pub sections: BTreeMap<String, String>,
}

impl GeneralSettings {
    pub fn validate(&self) -> AppResult<GeneralSettings> {
        let mut e = Errors::new();
        let default_lang = lang_code(&mut e, "default_lang", &self.default_lang);
        let mut languages = Vec::new();
        for l in &self.languages {
            let l = lang_code(&mut e, "languages", l);
            if !languages.contains(&l) {
                languages.push(l);
            }
        }
        if languages.is_empty() || languages.len() > 8 {
            e.push("languages", "Da 1 a 8 lingue");
        }
        if !languages.contains(&default_lang) {
            e.push("default_lang", "La lingua predefinita deve essere tra le lingue attive");
        }

        let b = &self.boot;
        let boot = BootSettings {
            version: plain(&mut e, "boot.version", &b.version, 1, 30),
            build: plain(&mut e, "boot.build", &b.build, 1, 30),
            kernel: plain(&mut e, "boot.kernel", &b.kernel, 1, 60),
            cpu: b.cpu,
            ram: b.ram,
            user: plain(&mut e, "boot.user", &b.user, 1, 30),
            command: plain(&mut e, "boot.command", &b.command, 1, 80),
            kinds: b.kinds.clone(),
        };
        in_range(&mut e, "boot.cpu", b.cpu as i64, 1, 4096);
        in_range(&mut e, "boot.ram", b.ram as i64, 1, 16_777_216);
        if b.kinds.len() > 40 || !b.kinds.iter().all(|k| matches!(k.as_str(), "i" | "o" | "l")) {
            e.push("boot.kinds", "Valori ammessi: i, o, l (max 40)");
        }

        if self.levels.is_empty() || self.levels.len() > 8 {
            e.push("levels", "Da 1 a 8 livelli");
        }
        let mut levels = Vec::new();
        for l in &self.levels {
            in_range(&mut e, "levels.min", l.min as i64, 0, 100);
            let key = l.key.trim().to_string();
            if key.is_empty() || key.len() > 20 || !key.chars().all(|c| c.is_ascii_alphanumeric()) {
                e.push("levels.key", "Chiave non valida");
            }
            levels.push(LevelRule { min: l.min, key });
        }
        if !levels.iter().any(|l| l.min == 0) {
            e.push("levels", "Serve un livello con soglia 0");
        }
        levels.sort_by(|a, b| b.min.cmp(&a.min));

        let mut dark = Vec::new();
        let mut light = Vec::new();
        if self.accents.dark.is_empty() || self.accents.dark.len() > 8 {
            e.push("accents.dark", "Da 1 a 8 colori");
        }
        if self.accents.light.is_empty() || self.accents.light.len() > 8 {
            e.push("accents.light", "Da 1 a 8 colori");
        }
        for c in &self.accents.dark {
            dark.push(hex_color(&mut e, "accents.dark", c));
        }
        for c in &self.accents.light {
            light.push(hex_color(&mut e, "accents.light", c));
        }

        let mut sections = BTreeMap::new();
        if self.sections.len() > 30 {
            e.push("sections", "Troppe sezioni");
        }
        for (k, v) in &self.sections {
            let key_ok = !k.is_empty()
                && k.len() <= 30
                && k.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
            let val_ok = v.len() >= 2
                && v.len() <= 31
                && v.starts_with('#')
                && v[1..].chars().enumerate().all(|(i, c)| {
                    c.is_ascii_lowercase() || (i > 0 && (c.is_ascii_digit() || c == '-' || c == '_'))
                });
            if !key_ok || !val_ok {
                e.push("sections", "Sezione non valida");
                continue;
            }
            sections.insert(k.clone(), v.clone());
        }

        e.finish()?;
        Ok(GeneralSettings {
            default_lang,
            languages,
            boot,
            levels,
            accents: Accents { dark, light },
            sections,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SeoSettings {
    #[serde(default)]
    pub site_title: BTreeMap<String, String>,
    #[serde(default)]
    pub site_description: BTreeMap<String, String>,
    #[serde(default)]
    pub og_title: BTreeMap<String, String>,
    #[serde(default)]
    pub og_description: BTreeMap<String, String>,
    #[serde(default)]
    pub keywords: BTreeMap<String, String>,
    #[serde(default)]
    pub og_image_media: Option<Uuid>,
    #[serde(default)]
    pub theme_color: Option<String>,
    #[serde(default = "default_true")]
    pub robots_index: bool,
}

impl SeoSettings {
    pub fn validate(&self) -> AppResult<SeoSettings> {
        let mut e = Errors::new();
        let site_title = i18n_map(&mut e, "site_title", &self.site_title, true, 70, false);
        let site_description = i18n_map(&mut e, "site_description", &self.site_description, false, 320, false);
        let og_title = i18n_map(&mut e, "og_title", &self.og_title, false, 90, false);
        let og_description = i18n_map(&mut e, "og_description", &self.og_description, false, 300, false);
        let keywords = i18n_map(&mut e, "keywords", &self.keywords, false, 300, false);
        let theme_color = match &self.theme_color {
            Some(c) if !c.trim().is_empty() => Some(hex_color(&mut e, "theme_color", c)),
            _ => None,
        };
        e.finish()?;
        Ok(SeoSettings {
            site_title,
            site_description,
            og_title,
            og_description,
            keywords,
            og_image_media: self.og_image_media,
            theme_color,
            robots_index: self.robots_index,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiTextItem {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiTextsInput {
    pub lang: String,
    pub items: Vec<UiTextItem>,
}

pub struct CleanUiTexts {
    pub lang: String,
    pub items: Vec<(String, String)>,
}

impl UiTextsInput {
    pub fn validate(&self) -> AppResult<CleanUiTexts> {
        let mut e = Errors::new();
        let lang = lang_code(&mut e, "lang", &self.lang);
        if self.items.is_empty() || self.items.len() > 500 {
            e.push("items", "Da 1 a 500 voci");
        }
        let mut items = Vec::new();
        for it in &self.items {
            let key_ok = !it.key.is_empty()
                && it.key.len() <= 80
                && it.key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-')
                && !it.key.starts_with('.')
                && !it.key.ends_with('.')
                && !it.key.contains("..");
            if !key_ok {
                e.push("items.key", "Chiave non valida");
                continue;
            }
            if it.value.chars().count() > 2000 {
                e.push("items.value", "Testo troppo lungo");
                continue;
            }
            let value = if it.key == "locale" {
                let cs: Vec<char> = it.value.chars().collect();
                let ok = cs.len() == 5
                    && cs[2] == '-'
                    && cs[..2].iter().all(|c| c.is_ascii_lowercase())
                    && cs[3..].iter().all(|c| c.is_ascii_uppercase());
                if !ok {
                    e.push("items.value", "Locale non valido (es. it-IT)");
                }
                it.value.clone()
            } else {
                sanitize_inline_html(&it.value)
            };
            items.push((it.key.clone(), value));
        }
        e.finish()?;
        Ok(CleanUiTexts { lang, items })
    }
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct AuditEntry {
    pub id: i64,
    pub at: DateTime<Utc>,
    pub severity: String,
    pub actor_id: Option<Uuid>,
    pub actor_login: Option<String>,
    pub action: String,
    pub entity: Option<String>,
    pub entity_id: Option<String>,
    pub ip: Option<String>,
    pub details: Json<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct SessionView {
    pub id: Uuid,
    pub user_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn base_project() -> ProjectInput {
        ProjectInput {
            slug: "demo".into(),
            name: None,
            title: map(&[("it", "Demo")]),
            subtitle: BTreeMap::new(),
            description: BTreeMap::new(),
            year_label: None,
            hue: 100,
            status: "active".into(),
            github_url: Some("https://github.com/x/y".into()),
            demo_url: None,
            started_on: None,
            ended_on: None,
            visible: true,
            technology_ids: vec![3, 1, 3],
            images: vec![],
        }
    }

    #[test]
    fn project_validation_normalizes() {
        let clean = base_project().validate();
        assert!(clean.is_ok());
        if let Ok(c) = clean {
            assert_eq!(c.technology_ids, vec![1, 3]);
        }
    }

    #[test]
    fn project_rejects_bad_input() {
        let mut p = base_project();
        p.github_url = Some("javascript:alert(1)".into());
        assert!(p.validate().is_err());
        let mut p = base_project();
        p.hue = 400;
        assert!(p.validate().is_err());
        let mut p = base_project();
        p.status = "weird".into();
        assert!(p.validate().is_err());
        let mut p = base_project();
        p.slug = "../etc".into();
        assert!(p.validate().is_err());
        let mut p = base_project();
        p.title = BTreeMap::new();
        assert!(p.validate().is_err());
    }

    #[test]
    fn profile_parts_must_match_name() {
        let mut p = ProfileInput {
            display_name: "zNxki_".into(),
            name_parts: vec!["z".into(), "Nxk".into(), "i_".into()],
            first_name: None,
            last_name: None,
            job_title: BTreeMap::new(),
            headline: BTreeMap::new(),
            bio_lead: BTreeMap::new(),
            bio_paragraphs: BTreeMap::new(),
            email: "a@b.it".into(),
            availability: "available".into(),
            availability_text: BTreeMap::new(),
        };
        assert!(p.validate().is_ok());
        p.name_parts = vec!["z".into(), "Nxk".into(), "x".into()];
        assert!(p.validate().is_err());
        p.name_parts = vec!["zNxki_".into()];
        assert!(p.validate().is_err());
    }

    #[test]
    fn email_contact_is_derived_from_profile() {
        let c = ContactInput {
            icon: "email".into(),
            label: "Email".into(),
            value: "evil@x.it".into(),
            href: "javascript:1".into(),
            visible: false,
        };
        let r = c.validate("real@x.it");
        assert!(r.is_ok());
        if let Ok(c) = r {
            assert_eq!(c.value, "real@x.it");
            assert_eq!(c.href, "mailto:real@x.it");
            assert!(c.visible);
        }
    }

    #[test]
    fn contact_rejects_unsafe_href() {
        let c = ContactInput {
            icon: "github".into(),
            label: "GitHub".into(),
            value: "@x".into(),
            href: "javascript:alert(1)".into(),
            visible: true,
        };
        assert!(c.validate("a@b.it").is_err());
    }

    #[test]
    fn general_settings_sorts_levels_and_checks_default_lang() {
        let g = GeneralSettings {
            default_lang: "en".into(),
            languages: vec!["en".into(), "it".into()],
            boot: BootSettings {
                version: "2.6".into(),
                build: "2026.11".into(),
                kernel: "6.1.0".into(),
                cpu: 8,
                ram: 16384,
                user: "guest".into(),
                command: "startx".into(),
                kinds: vec!["i".into(), "l".into()],
            },
            levels: vec![
                LevelRule { min: 0, key: "growing".into() },
                LevelRule { min: 85, key: "advanced".into() },
                LevelRule { min: 72, key: "solid".into() },
            ],
            accents: Accents {
                dark: vec!["#9fd3ff".into()],
                light: vec!["#0b6bcb".into()],
            },
            sections: [("about".to_string(), "#about".to_string())].into_iter().collect(),
        };
        let ok = g.validate();
        assert!(ok.is_ok());
        if let Ok(v) = ok {
            assert_eq!(v.levels[0].min, 85);
            assert_eq!(v.levels[2].min, 0);
        }
        let mut bad = g.clone();
        bad.default_lang = "fr".into();
        assert!(bad.validate().is_err());
        let mut bad = g.clone();
        bad.sections.insert("x".into(), "javascript:1".into());
        assert!(bad.validate().is_err());
        let mut bad = g;
        bad.levels.retain(|l| l.min != 0);
        assert!(bad.validate().is_err());
    }

    #[test]
    fn ui_texts_validation() {
        let ok = UiTextsInput {
            lang: "it".into(),
            items: vec![UiTextItem { key: "nav.about".into(), value: "chi <b>sono</b><script>".into() }],
        };
        let r = ok.validate();
        assert!(r.is_ok());
        if let Ok(c) = r {
            assert!(!c.items[0].1.contains("<script"));
        }
        let bad = UiTextsInput {
            lang: "it".into(),
            items: vec![UiTextItem { key: "a b".into(), value: "x".into() }],
        };
        assert!(bad.validate().is_err());
        let bad_locale = UiTextsInput {
            lang: "it".into(),
            items: vec![UiTextItem { key: "locale".into(), value: "xx".into() }],
        };
        assert!(bad_locale.validate().is_err());
    }

    #[test]
    fn role_roundtrip() {
        assert_eq!(Role::parse("ADMIN"), Some(Role::Admin));
        assert_eq!(Role::parse("USER"), Some(Role::User));
        assert_eq!(Role::parse("admin"), None);
        assert_eq!(Role::Admin.as_str(), "ADMIN");
    }

    #[test]
    fn i18n_pick_falls_back() {
        let i = I18n(map(&[("en", "hello")]));
        assert_eq!(i.pick("it", "en"), "hello");
        assert_eq!(i.pick("en", "en"), "hello");
        assert_eq!(I18n::default().pick("it", "en"), "");
    }
}