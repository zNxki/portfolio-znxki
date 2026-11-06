// backend/src/config.rs
use std::net::SocketAddr;
use std::path::PathBuf;

use url::Url;

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub public_url: Url,
    pub database_url: String,
    pub migration_database_url: Option<String>,
    pub db_max_connections: u32,
    pub github_client_id: String,
    pub github_client_secret: String,
    pub github_authorize_url: String,
    pub github_token_url: String,
    pub github_api_url: String,
    pub app_secret: Vec<u8>,
    pub admin_github_ids: Vec<i64>,
    pub frontend_dir: PathBuf,
    pub admin_dir: PathBuf,
    pub upload_dir: PathBuf,
    pub trust_proxy: bool,
    pub session_idle_secs: i64,
    pub session_absolute_secs: i64,
    pub session_rotate_secs: i64,
    pub rate_public_per_min: u32,
    pub rate_auth_per_min: u32,
    pub rate_admin_per_min: u32,
    pub max_json_bytes: usize,
    pub max_upload_bytes: usize,
}

#[derive(Debug)]
pub struct ConfigError(pub String);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "configurazione non valida: {}", self.0)
    }
}

impl std::error::Error for ConfigError {}

fn required(name: &str) -> Result<String, ConfigError> {
    match std::env::var(name) {
        Ok(v) if !v.trim().is_empty() => Ok(v.trim().to_string()),
        _ => Err(ConfigError(format!("variabile mancante: {name}"))),
    }
}

fn optional(name: &str, default: &str) -> String {
    match std::env::var(name) {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => default.to_string(),
    }
}

fn parse_num<T: std::str::FromStr>(name: &str, default: &str) -> Result<T, ConfigError> {
    optional(name, default)
        .parse::<T>()
        .map_err(|_| ConfigError(format!("valore numerico non valido: {name}")))
}

fn parse_bool(name: &str, default: bool) -> Result<bool, ConfigError> {
    let raw = optional(name, if default { "true" } else { "false" });
    match raw.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(ConfigError(format!("valore booleano non valido: {name}"))),
    }
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let public_url = Url::parse(&required("PUBLIC_URL")?)
            .map_err(|_| ConfigError("PUBLIC_URL non è un URL valido".into()))?;
        if public_url.scheme() != "https" && public_url.scheme() != "http" {
            return Err(ConfigError("PUBLIC_URL deve usare http o https".into()));
        }
        if public_url.host_str().is_none() {
            return Err(ConfigError("PUBLIC_URL senza host".into()));
        }

        let app_secret = required("APP_SECRET")?;
        if app_secret.len() < 32 {
            return Err(ConfigError(
                "APP_SECRET deve avere almeno 32 caratteri".into(),
            ));
        }

        let mut admin_github_ids = Vec::new();
        for part in optional("ADMIN_GITHUB_IDS", "").split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let id = part
                .parse::<i64>()
                .map_err(|_| ConfigError("ADMIN_GITHUB_IDS deve contenere ID numerici".into()))?;
            admin_github_ids.push(id);
        }

        if admin_github_ids.is_empty() {
            return Err(ConfigError(
                "ADMIN_GITHUB_IDS è obbligatorio: indica almeno un ID GitHub numerico".into(),
            ));
        }

        let bind_addr = optional("BIND_ADDR", "127.0.0.1:8080")
            .parse::<SocketAddr>()
            .map_err(|_| ConfigError("BIND_ADDR non valido".into()))?;

        Ok(Self {
            bind_addr,
            public_url,
            database_url: required("DATABASE_URL")?,
            migration_database_url: std::env::var("MIGRATION_DATABASE_URL")
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
            db_max_connections: parse_num("DB_MAX_CONNECTIONS", "10")?,
            github_client_id: required("GITHUB_CLIENT_ID")?,
            github_client_secret: required("GITHUB_CLIENT_SECRET")?,
            github_authorize_url: optional(
                "GITHUB_AUTHORIZE_URL",
                "https://github.com/login/oauth/authorize",
            ),
            github_token_url: optional(
                "GITHUB_TOKEN_URL",
                "https://github.com/login/oauth/access_token",
            ),
            github_api_url: optional("GITHUB_API_URL", "https://api.github.com"),
            app_secret: app_secret.into_bytes(),
            admin_github_ids,
            frontend_dir: PathBuf::from(optional("FRONTEND_DIR", "..")),
            admin_dir: PathBuf::from(optional("ADMIN_DIR", "./admin")),
            upload_dir: PathBuf::from(optional("UPLOAD_DIR", "./uploads")),
            trust_proxy: parse_bool("TRUST_PROXY", true)?,
            session_idle_secs: parse_num("SESSION_IDLE_SECS", "1800")?,
            session_absolute_secs: parse_num("SESSION_ABSOLUTE_SECS", "43200")?,
            session_rotate_secs: parse_num("SESSION_ROTATE_SECS", "900")?,
            rate_public_per_min: parse_num("RATE_PUBLIC_PER_MIN", "300")?,
            rate_auth_per_min: parse_num("RATE_AUTH_PER_MIN", "20")?,
            rate_admin_per_min: parse_num("RATE_ADMIN_PER_MIN", "240")?,
            max_json_bytes: parse_num("MAX_JSON_BYTES", "1048576")?,
            max_upload_bytes: parse_num("MAX_UPLOAD_BYTES", "8388608")?,
        })
    }

    pub fn cookie_secure(&self) -> bool {
        self.public_url.scheme() == "https"
    }

    pub fn origin(&self) -> String {
        self.public_url.origin().ascii_serialization()
    }

    pub fn redirect_uri(&self) -> String {
        format!("{}/auth/github/callback", self.origin())
    }
}
