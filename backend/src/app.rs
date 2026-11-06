// backend/src/app.rs
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::http::{HeaderValue, StatusCode};
use axum::middleware::from_fn_with_state;
use axum::Router;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tower_http::timeout::TimeoutLayer;

use crate::auth::session;
use crate::config::Config;
use crate::database::seed;
use crate::database::site::SiteCache;
use crate::error::{AppError, AppResult};
use crate::security::headers::{admin_csp, inline_script_hashes, public_csp};
use crate::security::ratelimit::RateLimiter;
use crate::state::{AppState, Inner};
use crate::{middleware, routes};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const PURGE_INTERVAL: Duration = Duration::from_secs(3600);
const RATE_WINDOW: Duration = Duration::from_secs(60);

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    let json = std::env::var("LOG_FORMAT")
        .map(|v| v.eq_ignore_ascii_case("json"))
        .unwrap_or(false);
    let _ = if json {
        builder.json().try_init()
    } else {
        builder.try_init()
    };
}

pub async fn connect(cfg: &Config) -> AppResult<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(cfg.db_max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&cfg.database_url)
        .await?;
    Ok(pool)
}

pub async fn migrate(cfg: &Config) -> AppResult<()> {
    let url = cfg
        .migration_database_url
        .as_deref()
        .unwrap_or(&cfg.database_url);
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(5))
        .connect(url)
        .await?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|e| AppError::internal(format!("migrazioni fallite: {e}")))?;
    pool.close().await;
    Ok(())
}

pub async fn build_state(cfg: Config, pool: PgPool) -> AppResult<AppState> {
    let template_path = cfg.frontend_dir.join("index.html");
    let index_template = tokio::fs::read_to_string(&template_path)
        .await
        .map_err(|e| {
            AppError::internal(format!(
                "lettura di {} fallita: {e}",
                template_path.display()
            ))
        })?;

    let csp_public = HeaderValue::from_str(&public_csp(&inline_script_hashes(&index_template)))
        .map_err(|_| AppError::internal("CSP pubblica non valida"))?;
    let csp_admin = HeaderValue::from_str(&admin_csp())
        .map_err(|_| AppError::internal("CSP admin non valida"))?;

    let http = reqwest::Client::builder()
        .user_agent("portfolio-backend")
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| AppError::internal(format!("client HTTP non creato: {e}")))?;

    Ok(AppState(Arc::new(Inner {
        cfg,
        pool,
        http,
        limiter: RateLimiter::new(),
        cache: SiteCache::new(),
        index_template,
        csp_public,
        csp_admin,
    })))
}

pub fn build_router(st: AppState) -> Router {
    routes::routes(&st.cfg)
        .layer(from_fn_with_state(st.clone(), middleware::csrf))
        .layer(from_fn_with_state(st.clone(), middleware::rate_limit))
        .layer(from_fn_with_state(st.clone(), middleware::session_layer))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            REQUEST_TIMEOUT,
        ))
        .layer(from_fn_with_state(st.clone(), middleware::security_headers))
        .layer(from_fn_with_state(st.clone(), middleware::request_context))
        .with_state(st)
}

pub fn spawn_maintenance(st: AppState) {
    let purge_state = st.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(PURGE_INTERVAL);
        loop {
            tick.tick().await;
            if let Err(e) = session::purge_expired(&purge_state.pool).await {
                tracing::warn!(error = %e, "pulizia sessioni fallita");
            }
        }
    });
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(RATE_WINDOW);
        loop {
            tick.tick().await;
            st.limiter.prune(RATE_WINDOW);
        }
    });
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("arresto in corso");
}

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let _ = dotenvy::dotenv();
    init_tracing();

    let cfg = Config::from_env()?;
    migrate(&cfg).await?;
    let pool = connect(&cfg).await?;
    seed::run(&pool, &cfg).await?;
    let demoted = crate::database::users::sync_admins(&pool, &cfg.admin_github_ids).await?;
    if demoted > 0 {
        tracing::warn!(
            demoted,
            "utenti rimossi da ADMIN_GITHUB_IDS declassati a USER"
        );
    }

    let bind = cfg.bind_addr;
    let st = build_state(cfg, pool).await?;
    spawn_maintenance(st.clone());
    let app = build_router(st);

    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(addr = %bind, "server in ascolto");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}
