use serde::Deserialize;
use url::Url;

use crate::config::Config;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawUser {
    id: i64,
    login: String,
    name: Option<String>,
    avatar_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GithubUser {
    pub id: i64,
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
}

pub fn authorize_url(cfg: &Config, state: &str, code_challenge: &str) -> AppResult<String> {
    let mut url = Url::parse(&cfg.github_authorize_url)
        .map_err(|_| AppError::internal("GITHUB_AUTHORIZE_URL non valido"))?;
    url.query_pairs_mut()
        .append_pair("client_id", &cfg.github_client_id)
        .append_pair("redirect_uri", &cfg.redirect_uri())
        .append_pair("state", state)
        .append_pair("code_challenge", code_challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("allow_signup", "false");
    Ok(url.to_string())
}

pub async fn exchange_code(
    http: &reqwest::Client,
    cfg: &Config,
    code: &str,
    code_verifier: &str,
) -> AppResult<String> {
    let redirect = cfg.redirect_uri();
    let response = http
        .post(&cfg.github_token_url)
        .header("Accept", "application/json")
        .form(&[
            ("client_id", cfg.github_client_id.as_str()),
            ("client_secret", cfg.github_client_secret.as_str()),
            ("code", code),
            ("redirect_uri", redirect.as_str()),
            ("code_verifier", code_verifier),
        ])
        .send()
        .await
        .map_err(|e| AppError::Upstream(format!("scambio del codice fallito: {}", e.without_url())))?;

    if !response.status().is_success() {
        return Err(AppError::Upstream(format!(
            "il provider ha risposto con stato {}",
            response.status().as_u16()
        )));
    }

    let body: TokenResponse = response
        .json()
        .await
        .map_err(|e| AppError::Upstream(format!("risposta token non valida: {}", e.without_url())))?;

    match body.access_token {
        Some(t) if !t.is_empty() => Ok(t),
        _ => {
            tracing::warn!(reason = body.error.as_deref().unwrap_or("sconosciuto"), "codice OAuth rifiutato");
            Err(AppError::BadRequest("Autenticazione non riuscita".into()))
        }
    }
}

fn valid_login(login: &str) -> bool {
    !login.is_empty()
        && login.len() <= 39
        && login.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

pub async fn fetch_user(
    http: &reqwest::Client,
    cfg: &Config,
    access_token: &str,
) -> AppResult<GithubUser> {
    let url = format!("{}/user", cfg.github_api_url.trim_end_matches('/'));
    let response = http
        .get(&url)
        .bearer_auth(access_token)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(|e| AppError::Upstream(format!("lettura profilo fallita: {}", e.without_url())))?;

    if !response.status().is_success() {
        return Err(AppError::Upstream(format!(
            "il provider ha risposto con stato {}",
            response.status().as_u16()
        )));
    }

    let raw: RawUser = response
        .json()
        .await
        .map_err(|e| AppError::Upstream(format!("profilo non valido: {}", e.without_url())))?;

    if raw.id <= 0 || !valid_login(&raw.login) {
        return Err(AppError::Upstream("profilo GitHub non valido".into()));
    }

    let name = raw
        .name
        .map(|n| n.chars().filter(|c| !c.is_control()).take(100).collect::<String>())
        .filter(|n| !n.trim().is_empty());
    let avatar_url = raw
        .avatar_url
        .filter(|u| u.starts_with("https://") && u.len() <= 500);

    Ok(GithubUser {
        id: raw.id,
        login: raw.login,
        name,
        avatar_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    pub fn test_config() -> Config {
        Config {
            bind_addr: "127.0.0.1:0".parse().unwrap_or_else(|_| ([127, 0, 0, 1], 0).into()),
            public_url: Url::parse("https://example.test").unwrap_or_else(|_| {
                Url::parse("https://localhost").expect("url statico valido")
            }),
            database_url: String::new(),
            migration_database_url: None,
            db_max_connections: 1,
            github_client_id: "cid".into(),
            github_client_secret: "super-secret".into(),
            github_authorize_url: "https://github.com/login/oauth/authorize".into(),
            github_token_url: "https://github.com/login/oauth/access_token".into(),
            github_api_url: "https://api.github.com".into(),
            app_secret: b"0123456789abcdef0123456789abcdef".to_vec(),
            admin_github_ids: vec![],
            frontend_dir: PathBuf::new(),
            admin_dir: PathBuf::new(),
            upload_dir: PathBuf::new(),
            trust_proxy: false,
            session_idle_secs: 1800,
            session_absolute_secs: 43200,
            session_rotate_secs: 900,
            rate_public_per_min: 300,
            rate_auth_per_min: 20,
            rate_admin_per_min: 240,
            max_json_bytes: 1_048_576,
            max_upload_bytes: 8_388_608,
        }
    }

    #[test]
    fn authorize_url_has_pkce_state_and_no_secret() {
        let cfg = test_config();
        let u = authorize_url(&cfg, "st4te", "ch4llenge").unwrap_or_default();
        assert!(u.starts_with("https://github.com/login/oauth/authorize?"));
        assert!(u.contains("state=st4te"));
        assert!(u.contains("code_challenge=ch4llenge"));
        assert!(u.contains("code_challenge_method=S256"));
        assert!(u.contains("client_id=cid"));
        assert!(u.contains("redirect_uri=https%3A%2F%2Fexample.test%2Fauth%2Fgithub%2Fcallback"));
        assert!(!u.contains("super-secret"));
    }

    #[test]
    fn login_validation() {
        assert!(valid_login("zNxki"));
        assert!(valid_login("a-b-1"));
        assert!(!valid_login(""));
        assert!(!valid_login("a b"));
        assert!(!valid_login("<script>"));
        assert!(!valid_login(&"a".repeat(40)));
    }
}