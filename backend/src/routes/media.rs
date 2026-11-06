use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get};
use axum::{Json, Router};
use serde_json::json;
use uuid::Uuid;

use super::{commit, ev};
use crate::audit;
use crate::auth::extractors::{AdminUser, ClientIp};
use crate::database::media;
use crate::error::{AppError, AppResult};
use crate::models::{Media, MediaOut};
use crate::security::tokens::sha256_hex;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/media", get(list).post(upload))
        .route("/api/admin/media/{id}", delete(remove))
}

pub fn sniff(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some(("image/png", "png"))
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(("image/jpeg", "jpg"))
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(("image/gif", "gif"))
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(("image/webp", "webp"))
    } else {
        None
    }
}

pub fn sanitize_name(raw: &str) -> Option<String> {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .filter(|c| !c.is_control() && *c != '<' && *c != '>')
        .take(120)
        .collect();
    let trimmed = cleaned.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

async fn list(State(st): State<AppState>, AdminUser(_): AdminUser) -> AppResult<Json<Vec<MediaOut>>> {
    let rows = media::list(&st.pool).await?;
    Ok(Json(rows.into_iter().map(MediaOut::from).collect()))
}

async fn upload(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AdminUser(user): AdminUser,
    mut multipart: Multipart,
) -> AppResult<(StatusCode, Json<MediaOut>)> {
    let mut data: Option<Vec<u8>> = None;
    let mut original: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| {
            if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
                AppError::PayloadTooLarge
            } else {
                AppError::bad_request("Upload non valido")
            }
        })?
    {
        if field.name() != Some("file") {
            continue;
        }
        original = field.file_name().and_then(sanitize_name);
        let bytes = field.bytes().await.map_err(|e| {
            if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
                AppError::PayloadTooLarge
            } else {
                AppError::bad_request("Upload non valido")
            }
        })?;
        if bytes.len() > st.cfg.max_upload_bytes {
            return Err(AppError::PayloadTooLarge);
        }
        data = Some(bytes.to_vec());
        break;
    }

    let bytes = data.ok_or_else(|| AppError::bad_request("File mancante"))?;
    let (mime, ext) = sniff(&bytes).ok_or(AppError::UnsupportedMedia)?;
    let id = Uuid::new_v4();
    let filename = format!("{id}.{ext}");
    let digest = sha256_hex(&bytes);

    tokio::fs::create_dir_all(&st.cfg.upload_dir)
        .await
        .map_err(|e| AppError::internal(format!("creazione cartella upload fallita: {e}")))?;
    let path = st.cfg.upload_dir.join(&filename);
    tokio::fs::write(&path, &bytes)
        .await
        .map_err(|e| AppError::internal(format!("scrittura upload fallita: {e}")))?;

    let outcome: AppResult<Media> = async {
        let mut tx = st.pool.begin().await?;
        let row = media::insert(
            &mut *tx,
            id,
            &filename,
            original.as_deref(),
            mime,
            bytes.len() as i64,
            &digest,
            Some(user.id),
        )
        .await?;
        audit::record(
            &mut *tx,
            ev("content.media.upload", &user, &ip)
                .entity("media", id)
                .details(json!({ "mime": mime, "size": bytes.len() })),
        )
        .await?;
        tx.commit().await?;
        Ok(row)
    }
    .await;

    match outcome {
        Ok(row) => Ok((StatusCode::CREATED, Json(MediaOut::from(row)))),
        Err(e) => {
            if let Err(rm) = tokio::fs::remove_file(&path).await {
                tracing::warn!(error = %rm, "rimozione file orfano fallita");
            }
            Err(e)
        }
    }
}

async fn remove(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    AdminUser(user): AdminUser,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    let mut tx = st.pool.begin().await?;
    let row = media::delete(&mut *tx, id).await?;
    audit::record(
        &mut *tx,
        ev("content.media.delete", &user, &ip)
            .entity("media", id)
            .details(json!({ "filename": row.filename })),
    )
    .await?;
    commit(&st, tx).await?;
    if let Err(e) = tokio::fs::remove_file(st.cfg.upload_dir.join(&row.filename)).await {
        tracing::warn!(error = %e, "rimozione file media fallita");
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_supported_formats() {
        assert_eq!(
            sniff(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0]),
            Some(("image/png", "png"))
        );
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), Some(("image/jpeg", "jpg")));
        assert_eq!(sniff(b"GIF89a...."), Some(("image/gif", "gif")));
        assert_eq!(sniff(b"RIFF\x00\x00\x00\x00WEBPVP8 "), Some(("image/webp", "webp")));
    }

    #[test]
    fn rejects_active_content() {
        assert_eq!(sniff(b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>"), None);
        assert_eq!(sniff(b"<html><script>alert(1)</script>"), None);
        assert_eq!(sniff(b"MZ\x90\x00"), None);
        assert_eq!(sniff(b""), None);
        assert_eq!(sniff(b"RIFF\x00\x00\x00\x00WAVEfmt "), None);
    }

    #[test]
    fn names_are_sanitized() {
        assert_eq!(sanitize_name("../../etc/passwd").as_deref(), Some("passwd"));
        assert_eq!(sanitize_name("a<b>.png").as_deref(), Some("ab.png"));
        assert_eq!(sanitize_name("C:\\x\\foto.jpg").as_deref(), Some("foto.jpg"));
        assert_eq!(sanitize_name("   ").as_deref(), None);
        assert_eq!(sanitize_name(&"a".repeat(300)).map(|s| s.len()), Some(120));
    }
}