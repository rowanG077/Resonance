use super::*;
use crate::model::{ModDraft, UPLOAD_AGE, fingerprint_valid, newer, part_length};
use axum::{body::to_bytes, routing::put};
use serde_json::{Value, json};
use std::io::{SeekFrom, Write};

#[derive(Deserialize)]
struct Start {
    metadata: ModDraft,
    size: u64,
    fingerprint: String,
}
#[derive(Clone)]
struct Upload {
    id: String,
    owner_id: i64,
    mod_id: String,
    metadata: ModDraft,
    size: u64,
    fingerprint: String,
    received: u64,
    next_part: u32,
    status: String,
    error: Option<String>,
    expires_at: i64,
}
impl Upload {
    fn public(&self) -> Value {
        let status = if self.status == "published" {
            "submitted"
        } else {
            self.status.as_str()
        };
        json!({"id":self.id,"mod_id":self.mod_id,"metadata":self.metadata,"size":self.size,
            "fingerprint":self.fingerprint,"received":self.received,"next_part":self.next_part,
            "part_size":PART_BYTES,"status":status,"error":self.error,"expires_at":self.expires_at,
            "page_url":format!("/mods/{}",self.mod_id)})
    }
    fn path(&self, app: &App) -> PathBuf {
        app.config
            .data
            .join("uploads")
            .join(format!("{}.zip", self.id))
    }
}
fn find(conn: &Connection, id: &str, owner: i64) -> Result<Upload> {
    let row = conn
        .query_row(
            "SELECT id,owner_id,mod_id,metadata,size,fingerprint,received,next_part,
                 status,error,expires_at FROM uploads WHERE id=?1 AND owner_id=?2",
            params![id, owner],
            |r| {
                Ok(Upload {
                    id: r.get(0)?,
                    owner_id: r.get(1)?,
                    mod_id: r.get(2)?,
                    metadata: serde_json::from_str(&r.get::<_, String>(3)?).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            3,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?,
                    size: r.get::<_, i64>(4)? as u64,
                    fingerprint: r.get(5)?,
                    received: r.get::<_, i64>(6)? as u64,
                    next_part: r.get(7)?,
                    status: r.get(8)?,
                    error: r.get(9)?,
                    expires_at: r.get(10)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| {
            Error(
                StatusCode::NOT_FOUND,
                "Upload not found in your account.".into(),
            )
        })?;
    Ok(row)
}
fn check_version(conn: &Connection, owner: i64, draft: &ModDraft) -> Result<()> {
    if let Some(id) = &draft.mod_id {
        let row: Option<(i64, String)> = conn
            .query_row(
                "SELECT m.owner_id,r.version FROM mods m JOIN releases r ON m.id=r.mod_id
                 WHERE m.id=?1 ORDER BY r.rowid DESC LIMIT 1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (author, version) =
            row.ok_or_else(|| Error(StatusCode::NOT_FOUND, "Mod not found.".into()))?;
        if author != owner {
            return Err(Error(
                StatusCode::FORBIDDEN,
                "You can only update your own mods.".into(),
            ));
        }
        newer(&draft.version, &version)?;
    }
    Ok(())
}
fn header_csrf(headers: &HeaderMap) -> Result<()> {
    csrf(
        headers,
        headers
            .get("x-csrf-token")
            .and_then(|v| v.to_str().ok())
            .unwrap_or(""),
    )
}
pub(super) fn routes() -> Router<Shared> {
    Router::new()
        .route(
            "/api/v1/uploads",
            post(start).layer(DefaultBodyLimit::max(32 * 1024)),
        )
        .route("/api/v1/uploads/active", get(active))
        .route("/api/v1/uploads/{id}", get(status))
        .route(
            "/api/v1/uploads/{id}/parts/{part}",
            put(part).layer(DefaultBodyLimit::disable()),
        )
        .route("/api/v1/uploads/{id}/complete", post(complete))
        .route("/api/v1/uploads/{id}/abort", post(abort))
}
async fn active(State(app): State<Shared>, headers: HeaderMap) -> Result<Response> {
    blocking(move || {
        let conn = db(&app)?;
        let owner = require_user(&conn, &headers)?;
        let id: Option<String> = conn
            .query_row(
                "SELECT id FROM uploads WHERE owner_id=?1 AND status IN ('uploading',
                 'validating') AND expires_at>?2",
                params![owner.id, now()],
                |r| r.get(0),
            )
            .optional()?;
        let upload = id
            .map(|id| find(&conn, &id, owner.id).map(|u| u.public()))
            .transpose()?;
        Ok(Json(json!({"upload":upload})).into_response())
    })
    .await
}
async fn status(
    State(app): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response> {
    blocking(move || {
        let conn = db(&app)?;
        Ok(Json(find(&conn, &id, require_user(&conn, &headers)?.id)?.public()).into_response())
    })
    .await
}
async fn start(
    State(app): State<Shared>,
    headers: HeaderMap,
    Timed(Json(start)): Timed<Json<Start>>,
) -> Result<Response> {
    header_csrf(&headers)?;
    let metadata = start.metadata.validate()?;
    if !fingerprint_valid(&start.fingerprint) {
        return Err(bad("Invalid file fingerprint. Select the ZIP again."));
    }
    blocking(move || {
        let mut conn = db(&app)?;
        let owner = require_user(&conn, &headers)?;
        UploadLimits::load(&conn, owner.id)?.check_size(start.size)?;
        // Persist attempt charges even when a later quota/version check fails.
        let rate = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        moderation::upload_access(&rate, owner.id)?;
        moderation::charge(&rate, owner.id, "starts", 1, 20, 3600)?;
        rate.commit()?;
        expire_for_owner(&app.config, &conn, owner.id)?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        moderation::allow_upload(&tx, owner.id, start.size)?;
        moderation::disk_space(&app.config, &tx, start.size)?;
        let releases: i64 = tx.query_row(
            "SELECT COUNT(*) FROM releases r JOIN mods m ON m.id=r.mod_id WHERE m.owner_id=?1",
            [owner.id],
            |r| r.get(0),
        )?;
        if releases >= 256 {
            return Err(bad(
                "Your account has reached 256 releases. Ask an administrator to remove \
                 old mods before uploading more.",
            ));
        }
        check_version(&tx, owner.id, &metadata)?;
        let active: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM uploads WHERE owner_id=?1 AND status IN
                 ('uploading','validating'))",
            [owner.id],
            |r| r.get(0),
        )?;
        if active {
            return Err(Error(
                StatusCode::CONFLICT,
                "Resume or cancel your current upload before starting another.".into(),
            ));
        }
        let id = random_token()[..24].to_owned();
        let mod_id = metadata
            .mod_id
            .clone()
            .unwrap_or_else(|| random_token()[..24].to_owned());
        tx.execute(
            "INSERT INTO uploads (id,owner_id,mod_id,metadata,size,fingerprint,
                 expires_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                id,
                owner.id,
                mod_id,
                serde_json::to_string(&metadata).map_err(internal)?,
                start.size as i64,
                start.fingerprint.to_lowercase(),
                now() + UPLOAD_AGE
            ],
        )?;
        let upload = find(&tx, &id, owner.id)?;
        let path = upload.path(&app);
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.sync_all()?;
        if let Err(error) = tx.commit() {
            let _ = std::fs::remove_file(&path);
            return Err(error.into());
        }
        Ok(Json(upload.public()).into_response())
    })
    .await
}
async fn part(
    State(app): State<Shared>,
    Path((id, part)): Path<(String, u32)>,
    request: Request,
) -> Result<Response> {
    let headers = request.headers().clone();
    header_csrf(&headers)?;
    // Authenticate before buffering even a single part.
    let auth_app = app.clone();
    let auth_headers = headers.clone();
    let auth_id = id.clone();
    let owner = blocking(move || {
        let mut conn = db(&auth_app)?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let owner = require_user(&tx, &auth_headers)?;
        let p = moderation::allow_upload(&tx, owner.id, 0)?;
        let upload = find(&tx, &auth_id, owner.id)?;
        p.limits.check_size(upload.size)?;
        part_length(upload.size, part)?;
        // Reserve a full body limit even for malformed or oversized part requests.
        moderation::charge(
            &tx,
            owner.id,
            "bytes",
            PART_BYTES,
            p.daily_mib * crate::model::MIB,
            86400,
        )?;
        moderation::charge(&tx, owner.id, "parts", 1, 5000, 3600)?;
        tx.commit()?;
        Ok(owner.id)
    })
    .await?;
    let bytes = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        to_bytes(request.into_body(), PART_BYTES as usize),
    )
    .await
    .map_err(|_| {
        Error(
            StatusCode::REQUEST_TIMEOUT,
            "Upload part timed out. Retry the part.".into(),
        )
    })?
    .map_err(|_| {
        Error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "An upload part may contain at most 8 MiB.".into(),
        )
    })?;
    blocking(move || {
        let mut conn = db(&app)?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let p = moderation::allow_upload(&tx, owner, 0)?;
        let upload = find(&tx, &id, owner)?;
        if upload.status == "published" {
            return Ok(Json(upload.public()).into_response());
        }
        if upload.expires_at <= now() {
            return Err(Error(
                StatusCode::GONE,
                "This upload has expired. Start another upload.".into(),
            ));
        }
        if upload.status != "uploading" {
            return Err(Error(
                StatusCode::CONFLICT,
                "This upload is no longer accepting file parts.".into(),
            ));
        }
        p.limits.check_size(upload.size)?;
        if part < upload.next_part {
            return Ok(Json(upload.public()).into_response());
        }
        if part != upload.next_part {
            return Err(Error(
                StatusCode::CONFLICT,
                "Upload file parts in order.".into(),
            ));
        }
        if bytes.len() != part_length(upload.size, part)? {
            return Err(bad("The upload part has the wrong size."));
        }
        let digest = hash(&bytes);
        let first_part = part == 1;
        let last_part = part == upload.size.div_ceil(PART_BYTES) as u32;
        if (first_part && digest != upload.fingerprint[..64])
            || (last_part && digest != upload.fingerprint[64..])
        {
            return Err(bad(
                "Select the original ZIP to resume this upload, or cancel it and start another.",
            ));
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(upload.path(&app))?;
        if file.metadata()?.len() < upload.received {
            return Err(internal("Stored upload is truncated"));
        }
        // Reclaim uncommitted bytes before checking space: the reservation
        // still includes them until their receipt is committed in SQLite.
        file.set_len(upload.received)?;
        moderation::disk_space(&app.config, &tx, 0)?;
        file.seek(SeekFrom::Start(upload.received))?;
        file.write_all(&bytes)?;
        file.sync_data()?;
        tx.execute(
            "UPDATE uploads SET received=received+?1,next_part=next_part+1,expires_at=?2 WHERE id=?3",
            params![bytes.len() as i64, now() + UPLOAD_AGE, id],
        )?;
        let upload = find(&tx, &id, owner)?;
        tx.commit()?;
        Ok(Json(upload.public()).into_response())
    })
    .await
}
async fn complete(
    State(app): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response> {
    header_csrf(&headers)?;
    blocking(move || {
        let mut conn = db(&app)?;
        let owner = require_user(&conn, &headers)?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let p = moderation::allow_upload(&tx, owner.id, 0)?;
        let mut upload = find(&tx, &id, owner.id)?;
        if ["published", "validating"].contains(&upload.status.as_str()) {
            return Ok(Json(upload.public()).into_response());
        }
        if upload.status != "uploading" {
            return Err(bad(upload
                .error
                .unwrap_or_else(|| "This upload was cancelled.".into())));
        }
        if upload.expires_at <= now() {
            return Err(Error(StatusCode::GONE, "This upload has expired.".into()));
        }
        if upload.received != upload.size {
            return Err(bad("Upload all file parts before submitting the release."));
        }
        p.limits.check_size(upload.size)?;
        let permit = app.upload_slots.clone().try_acquire_owned().map_err(|_| {
            Error(
                StatusCode::TOO_MANY_REQUESTS,
                "Two archives are being checked. Please retry shortly.".into(),
            )
        })?;
        tx.execute(
            "UPDATE uploads SET status='validating',expires_at=?1 WHERE id=?2",
            params![now() + UPLOAD_AGE, id],
        )?;
        tx.commit()?;
        upload.status = "validating".into();
        let response = Json(upload.public()).into_response();
        // Launch before leaving this non-cancellable operation: a disconnected
        // request must not leave a committed validation without a worker.
        // Validation still runs separately so the response can return immediately.
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            if let Err(error) = submit_release(&app, &upload) {
                tracing::warn!(upload=%upload.id,error=%error.1,"Archive was not submitted");
                if let Err(cleanup_error) = fail_upload(&app, &upload, &error.1) {
                    tracing::error!(error=%cleanup_error.1,"Could not clean up failed upload");
                }
            }
        });
        Ok(response)
    })
    .await
}
fn fail_upload(app: &App, upload: &Upload, message: &str) -> Result<()> {
    let mut conn = db(app)?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if tx.execute(
        "UPDATE uploads SET status='failed',error=?1 WHERE id=?2 AND status='validating'",
        params![message, upload.id],
    )? == 1
    {
        tx.execute(
            "INSERT OR IGNORE INTO file_deletions VALUES (?1,?2,?3)",
            params![
                format!("{}.zip", upload.id),
                upload.owner_id,
                upload.size as i64
            ],
        )?;
    }
    tx.commit()?;
    moderation::collect_files(&app.config, &conn, Some(upload.owner_id))
}
fn submit_release(app: &App, upload: &Upload) -> Result<()> {
    let mut conn = db(app)?;
    let limits = UploadLimits::load(&conn, upload.owner_id)?;
    let mut file = std::fs::File::open(upload.path(app))?;
    if file.metadata()?.len() != upload.size {
        return Err(bad("Stored upload has the wrong size."));
    }
    let usage = validate_archive(std::io::BufReader::new(&mut file), limits)?;
    file.rewind()?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    let sha256 = hex::encode(digest.finalize());
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let policy = moderation::allow_upload(&tx, upload.owner_id, 0)?;
    let current = find(&tx, &upload.id, upload.owner_id)?;
    if current.status != "validating" {
        return Err(bad("The upload was cancelled."));
    }
    let limits = policy.limits;
    limits.check_size(upload.size)?;
    if usage.expanded > limits.expanded_bytes() || usage.entries > limits.entries() {
        return Err(Error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "The archive exceeds your current upload limits.".into(),
        ));
    }
    check_version(&tx, upload.owner_id, &upload.metadata)?;
    let d = &upload.metadata;
    tx.execute(
        "INSERT INTO mods (id,owner_id,title,summary,description,category)
                 VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO NOTHING",
        params![
            upload.mod_id,
            upload.owner_id,
            d.title,
            d.summary,
            d.description,
            d.category
        ],
    )?;
    tx.execute(
        "INSERT INTO releases (id,mod_id,version,resonance_version,content_version,
                 filename,sha256,size) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            upload.id,
            upload.mod_id,
            d.version,
            d.resonance_version,
            d.content_version,
            format!("{}.zip", upload.id),
            sha256,
            upload.size as i64
        ],
    )?;
    tx.execute(
        "INSERT INTO release_reviews (release_id,metadata) VALUES (?1,?2)",
        params![upload.id, serde_json::to_string(d).map_err(internal)?],
    )?;
    tx.execute(
        "UPDATE uploads SET status='published' WHERE id=?1",
        [&upload.id],
    )?;
    tx.commit()?;
    Ok(())
}
async fn abort(
    State(app): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response> {
    header_csrf(&headers)?;
    blocking(move || {
        let mut conn = db(&app)?;
        let owner = require_user(&conn, &headers)?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut upload = find(&tx, &id, owner.id)?;
        if upload.status == "published" {
            return Err(Error(
                StatusCode::CONFLICT,
                "This release has already been submitted for review.".into(),
            ));
        }
        tx.execute("UPDATE uploads SET status='aborted' WHERE id=?1", [&id])?;
        tx.execute(
            "INSERT OR IGNORE INTO file_deletions VALUES (?1,?2,?3)",
            params![format!("{}.zip", id), owner.id, upload.size as i64],
        )?;
        tx.commit()?;
        moderation::collect_files(&app.config, &conn, Some(owner.id))?;
        upload.status = "aborted".into();
        Ok(Json(upload.public()).into_response())
    })
    .await
}
fn expire(tx: &rusqlite::Transaction<'_>, owner: Option<i64>) -> Result<()> {
    let cutoff = now();
    tx.execute(
        "INSERT OR IGNORE INTO file_deletions SELECT id||'.zip',owner_id,size
                 FROM uploads WHERE (?1 IS NULL OR owner_id=?1)
                 AND expires_at<=?2 AND status!='published'",
        params![owner, cutoff],
    )?;
    tx.execute(
        "DELETE FROM uploads WHERE (?1 IS NULL OR owner_id=?1) AND expires_at<=?2",
        params![owner, cutoff],
    )?;
    Ok(())
}
fn expire_for_owner(config: &Config, conn: &Connection, owner: i64) -> Result<()> {
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    expire(&tx, Some(owner))?;
    tx.commit()?;
    moderation::collect_files(config, conn, Some(owner))
}
pub(super) fn cleanup(config: &Config, conn: &Connection) -> Result<()> {
    // The DB decides visibility; an archive on disk is never public on its own.
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    expire(&tx, None)?;
    tx.execute("DELETE FROM sessions WHERE expires_at<=?1", [now()])?;
    tx.execute("DELETE FROM auth_attempts WHERE reset_at<=?1", [now()])?;
    tx.execute("DELETE FROM upload_rates WHERE reset_at<=?1", [now()])?;
    // Recover files created just before a crash prevented their upload row committing.
    for entry in std::fs::read_dir(config.data.join("uploads"))? {
        let entry = entry?;
        let filename = entry.file_name().to_string_lossy().into_owned();
        if !filename.ends_with(".zip") || !entry.file_type()?.is_file() {
            continue;
        }
        let age = entry.metadata()?.modified()?.elapsed().unwrap_or_default();
        if age.as_secs() < UPLOAD_AGE as u64 {
            continue;
        }
        let known: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM uploads WHERE id||'.zip'=?1) OR EXISTS(SELECT
                 1 FROM releases WHERE filename=?1) OR EXISTS(SELECT 1 FROM file_deletions
                 WHERE filename=?1)",
            [&filename],
            |r| r.get(0),
        )?;
        if !known {
            // Orphans have no owner to charge. Leave failures on disk for the
            // next scan instead of preventing startup or unrelated uploads.
            if let Err(error) = std::fs::remove_file(entry.path()) {
                tracing::error!(%filename, %error, "Could not remove orphan upload; will retry");
            }
        }
    }
    tx.commit()?;
    moderation::collect_files(config, conn, None)
}
