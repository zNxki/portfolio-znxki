use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct FieldError {
    pub field: String,
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("validation failed")]
    Validation(Vec<FieldError>),
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("not found")]
    NotFound,
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("too many requests")]
    TooManyRequests { retry_after: u64 },
    #[error("payload too large")]
    PayloadTooLarge,
    #[error("unsupported media type")]
    UnsupportedMedia,
    #[error("upstream error: {0}")]
    Upstream(String),
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("internal error: {0}")]
    Internal(String),
}

pub type AppResult<T> = Result<T, AppError>;

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: ErrorInner<'a>,
}

#[derive(Serialize)]
struct ErrorInner<'a> {
    code: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    fields: Option<&'a [FieldError]>,
}

fn pg_code(err: &sqlx::Error) -> Option<String> {
    err.as_database_error()
        .and_then(|d| d.code())
        .map(|c| c.to_string())
}

impl AppError {
    pub fn internal(msg: impl Into<String>) -> Self {
        AppError::Internal(msg.into())
    }

    pub fn bad_request(msg: impl Into<String>) -> Self {
        AppError::BadRequest(msg.into())
    }

    pub fn status(&self) -> StatusCode {
        match self {
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            AppError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppError::Forbidden => StatusCode::FORBIDDEN,
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::Conflict(_) => StatusCode::CONFLICT,
            AppError::TooManyRequests { .. } => StatusCode::TOO_MANY_REQUESTS,
            AppError::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            AppError::UnsupportedMedia => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            AppError::Upstream(_) => StatusCode::BAD_GATEWAY,
            AppError::Database(e) => match e {
                sqlx::Error::RowNotFound => StatusCode::NOT_FOUND,
                other => match pg_code(other).as_deref() {
                    Some("23505") | Some("23503") => StatusCode::CONFLICT,
                    Some("23514") | Some("22P02") | Some("22001") => StatusCode::UNPROCESSABLE_ENTITY,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                },
            },
            AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn public_parts(&self) -> (&'static str, String) {
        match self {
            AppError::BadRequest(m) => ("bad_request", m.clone()),
            AppError::Validation(_) => ("validation_failed", "Dati non validi".to_string()),
            AppError::Unauthorized => ("unauthorized", "Autenticazione richiesta".to_string()),
            AppError::Forbidden => ("forbidden", "Operazione non consentita".to_string()),
            AppError::NotFound => ("not_found", "Risorsa non trovata".to_string()),
            AppError::Conflict(m) => ("conflict", m.clone()),
            AppError::TooManyRequests { .. } => {
                ("rate_limited", "Troppe richieste, riprova più tardi".to_string())
            }
            AppError::PayloadTooLarge => ("payload_too_large", "Contenuto troppo grande".to_string()),
            AppError::UnsupportedMedia => {
                ("unsupported_media", "Formato non supportato".to_string())
            }
            AppError::Upstream(_) => (
                "upstream_error",
                "Servizio di autenticazione non disponibile".to_string(),
            ),
            AppError::Database(e) => match e {
                sqlx::Error::RowNotFound => ("not_found", "Risorsa non trovata".to_string()),
                other => match pg_code(other).as_deref() {
                    Some("23505") => ("conflict", "Esiste già un elemento con gli stessi dati".to_string()),
                    Some("23503") => (
                        "conflict",
                        "Elemento collegato ad altri dati, operazione non possibile".to_string(),
                    ),
                    Some("23514") | Some("22P02") | Some("22001") => {
                        ("validation_failed", "Dati non validi".to_string())
                    }
                    _ => ("internal_error", "Errore interno".to_string()),
                },
            },
            AppError::Internal(_) => ("internal_error", "Errore interno".to_string()),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match &self {
            AppError::Internal(m) => tracing::error!(error = %m, "internal error"),
            AppError::Upstream(m) => tracing::warn!(error = %m, "upstream error"),
            AppError::Database(e) => {
                if self.status() == StatusCode::INTERNAL_SERVER_ERROR {
                    tracing::error!(error = %e, "database error");
                }
            }
            _ => {}
        }

        let status = self.status();
        let (code, message) = self.public_parts();
        let fields = match &self {
            AppError::Validation(f) => Some(f.as_slice()),
            _ => None,
        };
        let body = ErrorBody {
            error: ErrorInner {
                code,
                message: &message,
                fields,
            },
        };
        let mut response = (status, Json(body)).into_response();
        if let AppError::TooManyRequests { retry_after } = &self {
            if let Ok(v) = HeaderValue::from_str(&retry_after.to_string()) {
                response.headers_mut().insert(header::RETRY_AFTER, v);
            }
        }
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        response
    }
}