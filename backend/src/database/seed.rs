use std::collections::BTreeMap;
use std::path::Path;

use chrono::NaiveDate;
use serde_json::{json, Value};
use sqlx::PgPool;

use super::{contacts, experiences, profile, projects, settings, site, technologies};
use crate::audit::{self, Event};
use crate::config::Config;
use crate::error::{AppError, AppResult};
use crate::models::{
    CleanContact, CleanExperience, CleanProfile, CleanProject, CleanTechnology, GeneralSettings,
    I18n, I18nList, SeoSettings,
};

async fn read_json(path: &Path) -> AppResult<Value> {
    let raw = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| AppError::internal(format!("lettura di {} fallita: {e}", path.display())))?;
    serde_json::from_str(&raw)
        .map_err(|e| AppError::internal(format!("JSON non valido in {}: {e}", path.display())))
}

fn str_at<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut cur = v;
    for p in path {
        cur = cur.get(*p)?;
    }
    cur.as_str()
}

fn per_lang<F>(dicts: &BTreeMap<String, Value>, f: F) -> BTreeMap<String, String>
where
    F: Fn(&Value) -> Option<String>,
{
    dicts
        .iter()
        .filter_map(|(lang, dict)| {
            f(dict)
                .filter(|s| !s.trim().is_empty())
                .map(|s| (lang.clone(), s))
        })
        .collect()
}

fn valid_lang(l: &str) -> bool {
    l.len() == 2 && l.chars().all(|c| c.is_ascii_lowercase())
}

fn parse_month(s: &str) -> Option<NaiveDate> {
    let first = s.split_whitespace().next()?;
    let (m, y) = first.split_once('/')?;
    NaiveDate::from_ymd_opt(y.trim().parse().ok()?, m.trim().parse().ok()?, 1)
}

fn parse_period(label: &str) -> (Option<NaiveDate>, Option<NaiveDate>) {
    let mut parts = label.split(" - ").map(str::trim);
    let start = parts.next().and_then(parse_month);
    let end = parts.next().filter(|p| !p.is_empty()).and_then(parse_month);
    (start, end)
}

fn default_job_title(lang: &str) -> Option<&'static str> {
    match lang {
        "it" => Some("Sviluppatore web"),
        "en" => Some("Web developer"),
        _ => None,
    }
}

pub async fn run(pool: &PgPool, cfg: &Config) -> AppResult<()> {
    if profile::exists(pool).await? {
        return Ok(());
    }

    let site_json = read_json(&cfg.frontend_dir.join("data").join("site.json")).await?;

    let default_lang = str_at(&site_json, &["defaultLang"])
        .unwrap_or("en")
        .to_string();
    let languages: Vec<String> = site_json["languages"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if languages.is_empty() || !languages.iter().all(|l| valid_lang(l)) {
        return Err(AppError::internal("languages in site.json non valido"));
    }

    let mut dicts: BTreeMap<String, Value> = BTreeMap::new();
    for l in &languages {
        let path = cfg.frontend_dir.join("lang").join(format!("{l}.json"));
        dicts.insert(l.clone(), read_json(&path).await?);
    }

    let general_raw = json!({
        "default_lang": default_lang,
        "languages": languages,
        "boot": site_json["boot"],
        "levels": site_json["levels"],
        "accents": site_json["accents"],
        "sections": site_json["sections"],
    });
    let general: GeneralSettings = serde_json::from_value(general_raw)
        .map_err(|e| AppError::internal(format!("site.json non valido: {e}")))?;
    let general = general.validate()?;

    let display_name = str_at(&site_json, &["name", "full"])
        .ok_or_else(|| AppError::internal("name.full mancante in site.json"))?
        .to_string();
    let name_parts: Vec<String> = site_json["name"]["parts"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    let contact_items: Vec<Value> = site_json["contacts"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let email = contact_items
        .iter()
        .find(|c| c["icon"].as_str() == Some("email"))
        .and_then(|c| c["value"].as_str())
        .ok_or_else(|| AppError::internal("contatto email mancante in site.json"))?
        .to_string();

    let job_title: BTreeMap<String, String> = languages
        .iter()
        .filter_map(|l| default_job_title(l).map(|t| (l.clone(), t.to_string())))
        .collect();
    let bio_paragraphs: BTreeMap<String, Vec<String>> = dicts
        .iter()
        .filter_map(|(lang, dict)| {
            dict["about"]["p"].as_array().map(|a| {
                (
                    lang.clone(),
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect(),
                )
            })
        })
        .collect();

    let clean_profile = CleanProfile {
        display_name,
        name_parts,
        first_name: None,
        last_name: None,
        job_title: I18n(job_title),
        headline: I18n(per_lang(&dicts, |d| {
            str_at(d, &["hero", "text"]).map(str::to_string)
        })),
        bio_lead: I18n(per_lang(&dicts, |d| {
            str_at(d, &["about", "lead"]).map(str::to_string)
        })),
        bio_paragraphs: I18nList(bio_paragraphs),
        email,
        availability: "available".to_string(),
        availability_text: I18n(per_lang(&dicts, |d| {
            str_at(d, &["tag"]).map(str::to_string)
        })),
    };

    let mut tx = pool.begin().await?;

    profile::upsert(&mut *tx, None, &clean_profile).await?;

    for c in contact_items
        .iter()
        .filter(|c| c["icon"].as_str() != Some("email"))
    {
        let clean = CleanContact {
            icon: c["icon"].as_str().unwrap_or_default().to_string(),
            label: c["label"].as_str().unwrap_or_default().to_string(),
            value: c["value"].as_str().unwrap_or_default().to_string(),
            href: c["href"].as_str().unwrap_or_default().to_string(),
            visible: true,
        };
        contacts::create(&mut *tx, None, &clean).await?;
    }

    let marquee: Vec<String> = site_json["marquee"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let skills: Vec<Value> = site_json["skills"].as_array().cloned().unwrap_or_default();
    let skill_names: Vec<String> = skills
        .iter()
        .filter_map(|s| s["name"].as_str().map(str::to_string))
        .collect();

    for s in &skills {
        let name = match s["name"].as_str() {
            Some(n) => n.to_string(),
            None => continue,
        };
        let level = s["level"].as_i64().unwrap_or(0).clamp(0, 100) as i32;
        let first_word = name.split(' ').next().unwrap_or("").to_string();
        let (in_marquee, marquee_label) = if marquee.contains(&name) {
            (true, None)
        } else if !first_word.is_empty()
            && first_word != name
            && marquee.contains(&first_word)
            && !skill_names.contains(&first_word)
        {
            (true, Some(first_word))
        } else {
            (false, None)
        };
        let description = per_lang(&dicts, |d| {
            str_at(d, &["skills", name.as_str()]).map(str::to_string)
        });
        let clean = CleanTechnology {
            name,
            marquee_label,
            icon: None,
            level,
            description: I18n(description),
            in_marquee,
            visible: true,
        };
        technologies::create(&mut *tx, None, &clean).await?;
    }

    let experience_slugs: Vec<String> = site_json["experience"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    for slug in &experience_slugs {
        let mut title = per_lang(&dicts, |d| {
            str_at(d, &["expItems", slug.as_str(), "t"]).map(str::to_string)
        });
        if title.is_empty() {
            title.insert(default_lang.clone(), slug.clone());
        }
        let subtitle = per_lang(&dicts, |d| {
            str_at(d, &["expItems", slug.as_str(), "s"]).map(str::to_string)
        });
        let clean = CleanExperience {
            slug: slug.clone(),
            kind: "personal".to_string(),
            title: I18n(title),
            subtitle: I18n(subtitle),
            role: I18n::default(),
            organization: None,
            description: I18n::default(),
            start_date: None,
            end_date: None,
            visible: true,
        };
        experiences::create(&mut *tx, None, &clean).await?;
    }

    let project_items: Vec<Value> = site_json["projects"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for p in &project_items {
        let slug = match p["id"].as_str() {
            Some(s) => s.to_string(),
            None => continue,
        };
        let name = p["name"].as_str().map(str::to_string);
        let year = p["year"].as_str().unwrap_or("").to_string();
        let hue = p["hue"].as_i64().unwrap_or(200).clamp(0, 359) as i32;

        let entry_for = |dict: &Value| -> Option<Value> {
            let items = dict.get("projectItems")?;
            if let Some(direct) = items.get(&slug) {
                return Some(direct.clone());
            }
            let wanted = name.as_deref()?;
            items
                .as_object()?
                .values()
                .find(|v| v["t"].as_str() == Some(wanted))
                .cloned()
        };

        let mut title = per_lang(&dicts, |d| {
            entry_for(d).and_then(|e| e["t"].as_str().map(str::to_string))
        });
        if title.is_empty() {
            title.insert(
                default_lang.clone(),
                name.clone().unwrap_or_else(|| slug.clone()),
            );
        }
        let subtitle = per_lang(&dicts, |d| {
            entry_for(d).and_then(|e| e["s"].as_str().map(str::to_string))
        });

        let (started_on, ended_on) = parse_period(&year);
        let status = if year.trim_end().ends_with('-') {
            "active"
        } else {
            "completed"
        };
        let clean = CleanProject {
            slug,
            name,
            title: I18n(title),
            subtitle: I18n(subtitle),
            description: I18n::default(),
            year_label: None,
            hue,
            status: status.to_string(),
            github_url: None,
            demo_url: None,
            started_on,
            ended_on,
            visible: true,
            technology_ids: Vec::new(),
            images: Vec::new(),
        };
        projects::create(&mut *tx, None, &clean).await?;
    }

    for (lang, dict) in &dicts {
        let items: Vec<(String, String)> = site::flatten(dict)
            .into_iter()
            .filter(|(k, _)| !site::is_derived_key(k))
            .collect();
        settings::upsert_ui_texts(&mut *tx, lang, &items).await?;
    }

    let titles = per_lang(&dicts, |d| {
        str_at(d, &["meta", "title"]).map(str::to_string)
    });
    let descriptions = per_lang(&dicts, |d| {
        str_at(d, &["meta", "description"]).map(str::to_string)
    });
    let seo = SeoSettings {
        site_title: titles.clone(),
        site_description: descriptions.clone(),
        og_title: titles,
        og_description: descriptions,
        keywords: BTreeMap::new(),
        og_image_media: None,
        theme_color: None,
        robots_index: true,
    };
    let seo = seo.validate()?;

    settings::put(&mut *tx, "general", &general, None).await?;
    settings::put(&mut *tx, "seo", &seo, None).await?;

    audit::record(
        &mut *tx,
        Event::new("system.seed").details(json!({
            "languages": general.languages,
            "projects": project_items.len(),
            "technologies": skills.len(),
            "experiences": experience_slugs.len()
        })),
    )
    .await?;

    tx.commit().await?;
    tracing::info!("contenuti iniziali importati dal frontend esistente");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn period_parsing() {
        let d = |y, m| NaiveDate::from_ymd_opt(y, m, 1);
        assert_eq!(parse_period("10/2026 -"), (d(2026, 10), None));
        assert_eq!(parse_period("11/2026"), (d(2026, 11), None));
        assert_eq!(parse_period("01/2025 - 03/2026"), (d(2025, 1), d(2026, 3)));
        assert_eq!(parse_period(""), (None, None));
        assert_eq!(parse_period("abc"), (None, None));
    }

    #[test]
    fn lang_codes() {
        assert!(valid_lang("it"));
        assert!(!valid_lang("../"));
        assert!(!valid_lang("ITA"));
        assert!(!valid_lang("i"));
    }

    #[test]
    fn job_titles_known_languages_only() {
        assert!(default_job_title("it").is_some());
        assert!(default_job_title("de").is_none());
    }
}
