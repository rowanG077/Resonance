use super::*;
use crate::model::MIB;
use serde_json::{Value, json};

impl UploadLimits {
    pub(super) fn load(conn: &Connection, user_id: i64) -> Result<Self> {
        Ok(conn
            .query_row(
                "SELECT zip_mib, expanded_mib FROM upload_limits WHERE user_id = ?1",
                [user_id],
                |r| {
                    Ok(Self {
                        zip_mib: r.get::<_, i64>(0)? as u64,
                        expanded_mib: r.get::<_, i64>(1)? as u64,
                    })
                },
            )
            .optional()?
            .unwrap_or_default())
    }
}

#[derive(Serialize)]
pub(super) struct Policy {
    pub status: String,
    pub quota_mib: u64,
    pub daily_mib: u64,
    pub used: u64,
    pub limits: UploadLimits,
}
pub(super) fn policy(conn: &Connection, id: i64) -> Result<Policy> {
    let (status, quota_mib, daily_mib) = conn
        .query_row(
            "SELECT status,quota_mib,daily_mib FROM author_policy WHERE user_id=?1",
            [id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get::<_, i64>(1)? as u64,
                    r.get::<_, i64>(2)? as u64,
                ))
            },
        )
        .optional()?
        .unwrap_or(("unapproved".into(), 512, 2048));
    let used = conn.query_row(
        "SELECT
            COALESCE((SELECT SUM(r.size) FROM releases r
                JOIN mods m ON m.id=r.mod_id
                JOIN release_reviews v ON v.release_id=r.id
                WHERE m.owner_id=?1 AND v.status!='rejected'),0)
            + COALESCE((SELECT SUM(size) FROM uploads
                WHERE owner_id=?1 AND status IN ('uploading','validating')),0)
            + COALESCE((SELECT SUM(size) FROM file_deletions WHERE owner_id=?1),0)",
        [id],
        |r| r.get::<_, i64>(0),
    )? as u64;
    Ok(Policy {
        status,
        quota_mib,
        daily_mib,
        used,
        limits: UploadLimits::load(conn, id)?,
    })
}
pub(super) fn upload_access(conn: &Connection, id: i64) -> Result<Policy> {
    let p = policy(conn, id)?;
    if p.status != "approved" {
        return Err(Error(StatusCode::FORBIDDEN,"An administrator must approve your upload access. Check Your mods for your account status.".into()));
    }
    Ok(p)
}
pub(super) fn allow_upload(conn: &Connection, id: i64, additional: u64) -> Result<Policy> {
    let p = upload_access(conn, id)?;
    if p.used.saturating_add(additional) > p.quota_mib * MIB {
        return Err(Error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "Your total storage allowance is full. Ask an administrator for a larger \
                 allowance or removal of an old release."
                .into(),
        ));
    }
    Ok(p)
}
pub(super) fn disk_space(config: &Config, conn: &Connection, additional: u64) -> Result<()> {
    let stats = rustix::fs::statvfs(&config.data).map_err(internal)?;
    let available = stats.f_bavail.saturating_mul(stats.f_frsize);
    let reserved = conn.query_row(
        "SELECT COALESCE(SUM(size-received),0) FROM uploads
         WHERE status IN ('uploading','validating')",
        [],
        |r| r.get::<_, i64>(0),
    )? as u64;
    if available
        < reserved
            .saturating_add(additional)
            .saturating_add(config.disk_reserve_bytes)
    {
        return Err(Error(
            StatusCode::INSUFFICIENT_STORAGE,
            "Uploads are paused because the server is low on free space. Please try again later."
                .into(),
        ));
    }
    Ok(())
}
// Charge before receiving data. Failed bodies, cancelled uploads and retries consume the allowance.
pub(super) fn charge(
    conn: &Connection,
    id: i64,
    kind: &str,
    amount: u64,
    maximum: u64,
    window: i64,
) -> Result<()> {
    let (used, reset): (u64, i64) = conn
        .query_row(
            "SELECT used,reset_at FROM upload_rates WHERE user_id=?1 AND kind=?2",
            params![id, kind],
            |r| Ok((r.get::<_, i64>(0)? as u64, r.get(1)?)),
        )
        .optional()?
        .unwrap_or((0, 0));
    let (used, reset) = if reset <= now() {
        (0, now() + window)
    } else {
        (used, reset)
    };
    if used.saturating_add(amount) > maximum {
        return Err(Error(
            StatusCode::TOO_MANY_REQUESTS,
            "Your upload rate allowance has been reached. Please retry after the allowance resets."
                .into(),
        ));
    }
    conn.execute(
        "INSERT INTO upload_rates VALUES (?1,?2,?3,?4) ON CONFLICT(user_id,
                 kind) DO UPDATE SET used=excluded.used,reset_at=excluded.reset_at",
        params![id, kind, (used + amount) as i64, reset],
    )?;
    Ok(())
}
fn audit(
    conn: &Connection,
    actor: i64,
    action: &str,
    target: impl ToString,
    note: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO moderation_log (actor_id,action,target,note) VALUES (?1,?2,?3,?4)",
        params![actor, action, target.to_string(), note],
    )?;
    Ok(())
}
pub(super) fn collect_files(config: &Config, conn: &Connection, owner: Option<i64>) -> Result<()> {
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    let files = tx
        .prepare(if owner.is_some() {
            "SELECT filename FROM file_deletions WHERE owner_id=?1"
        } else {
            "SELECT filename FROM file_deletions WHERE ?1 IS NULL"
        })?
        .query_map([owner], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for file in files {
        match std::fs::remove_file(config.data.join("uploads").join(&file)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::error!(filename=%file,error=%e,"Could not remove moderated upload; will retry");
                continue;
            }
        }
        tx.execute("DELETE FROM file_deletions WHERE filename=?1", [file])?;
    }
    tx.commit()?;
    Ok(())
}
pub(super) fn routes() -> Router<Shared> {
    Router::new()
        .route("/admin", get(page))
        .route("/admin/accounts/{id}", post(account))
        .route("/admin/accounts/{id}/role", post(role))
        .route("/admin/releases/{id}/review", post(review))
        .route("/admin/releases/{id}/download", get(review_download))
        .route("/admin/mods/{id}/delete", post(delete_mod))
        .route("/account/upload-access", post(request_access))
}
#[derive(Deserialize)]
struct AccessForm {
    csrf: String,
    reason: String,
}
async fn request_access(
    State(app): State<Shared>,
    headers: HeaderMap,
    Timed(Form(form)): Timed<Form<AccessForm>>,
) -> Result<Response> {
    csrf(&headers, &form.csrf)?;
    if !(10..=2000).contains(&form.reason.trim().len()) {
        return Err(bad(
            "Describe the mods you plan to upload in 10–2000 bytes.",
        ));
    }
    blocking(move || {
        let conn = db(&app)?;
        let owner = require_user(&conn, &headers)?;
        let changed = conn.execute(
            "INSERT INTO author_policy (user_id,reason) VALUES (?1,?2)
             ON CONFLICT(user_id) DO NOTHING",
            params![owner.id, form.reason.trim()],
        )?;
        if changed == 0 {
            return Err(Error(
                StatusCode::CONFLICT,
                "Your upload access has already been requested or reviewed.".into(),
            ));
        }
        Ok(Redirect::to("/dashboard").into_response())
    })
    .await
}
async fn page(State(app): State<Shared>, headers: HeaderMap) -> Result<Response> {
    blocking(move || {
        let conn = db(&app)?;
        let admin = require_admin(&conn, &headers)?;
        let mut accounts = vec![];
        let mut stmt = conn.prepare(
            "SELECT u.id,u.username,
                EXISTS(SELECT 1 FROM site_admins a WHERE a.user_id=u.id),
                COALESCE(p.reason,'')
             FROM users u LEFT JOIN author_policy p ON p.user_id=u.id
             ORDER BY u.id DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?,
                r.get::<_, bool>(2)?, r.get::<_, String>(3)?))
        })?;
        for row in rows {
            let (id, name, admin, reason) = row?;
            accounts.push(json!({
                "id": id, "username": name, "admin": admin,
                "reason": reason, "policy": policy(&conn, id)?,
            }));
        }
        let reviews = conn.prepare(
            "SELECT r.id,r.mod_id,r.version,r.size,u.username,v.status,v.metadata,v.note
             FROM releases r JOIN mods m ON m.id=r.mod_id
             JOIN users u ON u.id=m.owner_id JOIN release_reviews v ON v.release_id=r.id
             WHERE v.status!='rejected'
             ORDER BY (v.status='pending') DESC,r.rowid DESC",
        )?.query_map([], |r| {
            Ok(json!({
                "id": r.get::<_, String>(0)?, "mod_id": r.get::<_, String>(1)?,
                "version": r.get::<_, String>(2)?, "size": r.get::<_, i64>(3)?,
                "author": r.get::<_, String>(4)?, "status": r.get::<_, String>(5)?,
                "metadata": serde_json::from_str::<Value>(&r.get::<_, String>(6)?).unwrap_or(Value::Null),
                "note": r.get::<_, String>(7)?,
            }))
        })?.collect::<std::result::Result<Vec<_>, _>>()?;
        let mods = conn.prepare(
            "SELECT m.id,m.title,u.username FROM mods m JOIN users u ON u.id=m.owner_id
             ORDER BY m.rowid DESC",
        )?.query_map([], |r| {
            Ok(json!({
                "id": r.get::<_, String>(0)?, "title": r.get::<_, String>(1)?,
                "author": r.get::<_, String>(2)?,
            }))
        })?.collect::<std::result::Result<Vec<_>, _>>()?;
        let log = conn.prepare(
            "SELECT u.username,l.action,l.target,l.note,l.created_at
             FROM moderation_log l JOIN users u ON u.id=l.actor_id
             ORDER BY l.id DESC LIMIT 50",
        )?.query_map([], |r| {
            Ok(json!({
                "actor": r.get::<_, String>(0)?, "action": r.get::<_, String>(1)?,
                "target": r.get::<_, String>(2)?, "note": r.get::<_, String>(3)?,
                "time": r.get::<_, String>(4)?,
            }))
        })?.collect::<std::result::Result<Vec<_>, _>>()?;
        render(
            &app, &conn, &headers, Some(&admin), "admin.html",
            json!({"accounts": accounts, "reviews": reviews, "mods": mods, "log": log}),
        )
    })
    .await
}
#[derive(Deserialize)]
struct AccountForm {
    csrf: String,
    status: String,
    quota_mib: u64,
    daily_mib: u64,
    zip_mib: u64,
    expanded_mib: u64,
}
async fn account(
    State(app): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Timed(Form(form)): Timed<Form<AccountForm>>,
) -> Result<Response> {
    csrf(&headers, &form.csrf)?;
    if !["requested", "approved", "suspended"].contains(&form.status.as_str())
        || !(1..=1048576).contains(&form.quota_mib)
        || !(8..=1048576).contains(&form.daily_mib)
    {
        return Err(bad(
            "Choose an upload status and allowances between 1 and 1048576 MiB.",
        ));
    }
    let limits = UploadLimits {
        zip_mib: form.zip_mib,
        expanded_mib: form.expanded_mib,
    }
    .validate()?;
    blocking(move || {
        let mut conn = db(&app)?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let admin = require_admin(&tx, &headers)?;
        if !tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM users WHERE id=?1)",
            [id],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(Error(StatusCode::NOT_FOUND, "Account not found.".into()));
        }
        tx.execute(
            "INSERT INTO author_policy (user_id,status,quota_mib,daily_mib) VALUES
                 (?1,?2,?3,?4) ON CONFLICT(user_id) DO UPDATE SET status=excluded.status,
                 quota_mib=excluded.quota_mib,daily_mib=excluded.daily_mib",
            params![
                id,
                form.status,
                form.quota_mib as i64,
                form.daily_mib as i64
            ],
        )?;
        tx.execute(
            "INSERT INTO upload_limits VALUES (?1,?2,?3) ON CONFLICT(user_id)
             DO UPDATE SET zip_mib=excluded.zip_mib,expanded_mib=excluded.expanded_mib",
            params![id, limits.zip_mib as i64, limits.expanded_mib as i64],
        )?;
        if form.status != "approved" {
            tx.execute(
                "INSERT OR IGNORE INTO file_deletions SELECT id||'.zip',owner_id,size
                 FROM uploads WHERE owner_id=?1 AND status IN ('uploading','validating')",
                [id],
            )?;
            tx.execute(
                "UPDATE uploads SET status='aborted',error='Upload access was revoked.'
                 WHERE owner_id=?1 AND status IN ('uploading','validating')",
                [id],
            )?;
        }
        audit(
            &tx,
            admin.id,
            "account",
            id,
            &format!(
                "{}; storage {} MiB; daily {} MiB; ZIP {} MiB; expanded {} MiB",
                form.status, form.quota_mib, form.daily_mib, limits.zip_mib, limits.expanded_mib
            ),
        )?;
        tx.commit()?;
        collect_files(&app.config, &conn, Some(id))?;
        Ok(Redirect::to("/admin#accounts").into_response())
    })
    .await
}
#[derive(Deserialize)]
struct RoleForm {
    csrf: String,
    admin: bool,
}
async fn role(
    State(app): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Timed(Form(form)): Timed<Form<RoleForm>>,
) -> Result<Response> {
    csrf(&headers, &form.csrf)?;
    blocking(move || {
        let mut conn = db(&app)?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let admin = require_admin(&tx, &headers)?;
        change_role(&tx, id, form.admin)?;
        audit(
            &tx,
            admin.id,
            "admin-role",
            id,
            if form.admin { "granted" } else { "revoked" },
        )?;
        tx.commit()?;
        Ok(Redirect::to("/admin#accounts").into_response())
    })
    .await
}
pub(super) fn change_role(conn: &Connection, id: i64, grant: bool) -> Result<()> {
    if !conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM users WHERE id=?1)",
        [id],
        |r| r.get::<_, bool>(0),
    )? {
        return Err(Error(StatusCode::NOT_FOUND, "Account not found.".into()));
    }
    if grant {
        conn.execute("INSERT OR IGNORE INTO site_admins VALUES (?1)", [id])?;
    } else {
        let last: bool = conn.query_row(
            "SELECT (SELECT COUNT(*) FROM site_admins)=1 AND EXISTS(SELECT 1 FROM
                 site_admins WHERE user_id=?1)",
            [id],
            |r| r.get(0),
        )?;
        if last {
            return Err(bad(
                "Keep at least one administrator. Promote another account first.",
            ));
        }
        conn.execute("DELETE FROM site_admins WHERE user_id=?1", [id])?;
    }
    Ok(())
}
#[derive(Deserialize)]
struct ReviewForm {
    csrf: String,
    decision: String,
    note: String,
}
async fn review(
    State(app): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Timed(Form(form)): Timed<Form<ReviewForm>>,
) -> Result<Response> {
    csrf(&headers, &form.csrf)?;
    if !["approve", "reject"].contains(&form.decision.as_str()) || form.note.len() > 2000 {
        return Err(bad("Choose approve or reject and a note up to 2000 bytes."));
    }
    blocking(move || {
        let mut conn = db(&app)?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let admin = require_admin(&tx, &headers)?;
        let row: Option<(String, String, i64, u64, String)> = tx
            .query_row(
                "SELECT v.status,r.mod_id,m.owner_id,r.size,r.filename FROM release_reviews
                 v JOIN releases r ON r.id=v.release_id JOIN mods m ON m.id=r.mod_id
                 WHERE r.id=?1",
                [&id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get::<_, i64>(3)? as u64,
                        r.get(4)?,
                    ))
                },
            )
            .optional()?;
        let (status, mod_id, owner, size, filename) =
            row.ok_or_else(|| Error(StatusCode::NOT_FOUND, "Release not found.".into()))?;
        if status == "rejected" || (status == "approved" && form.decision == "approve") {
            return Err(Error(
                StatusCode::CONFLICT,
                "This release has already been reviewed.".into(),
            ));
        }
        let status = if form.decision == "approve" {
            // The stored release already counts toward quota; approval adds no data.
            upload_access(&tx, owner)?;
            if !app.config.data.join("uploads").join(&filename).is_file() {
                return Err(bad("The release archive is missing."));
            }
            "approved"
        } else {
            tx.execute(
                "INSERT OR IGNORE INTO file_deletions VALUES (?1,?2,?3)",
                params![filename, owner, size as i64],
            )?;
            "rejected"
        };
        tx.execute(
            "UPDATE release_reviews SET status=?1,reviewer_id=?2,note=?3 WHERE release_id=?4",
            params![status, admin.id, form.note.trim(), id],
        )?;
        // Public text comes from the latest approved release, never an unreviewed update.
        let latest: Option<String> = tx
            .query_row(
                "SELECT v.metadata FROM releases r JOIN release_reviews v ON v.release_id=r.id
                 WHERE r.mod_id=?1 AND v.status='approved' ORDER BY r.rowid DESC LIMIT
                 1",
                [&mod_id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(latest) = latest {
            let d: Value = serde_json::from_str(&latest).map_err(internal)?;
            tx.execute(
                "UPDATE mods SET title=?1,summary=?2,description=?3,category=?4 WHERE id=?5",
                params![
                    d["title"].as_str(),
                    d["summary"].as_str(),
                    d["description"].as_str(),
                    d["category"].as_str(),
                    mod_id
                ],
            )?;
        }
        audit(&tx, admin.id, status, &id, form.note.trim())?;
        tx.commit()?;
        collect_files(&app.config, &conn, Some(owner))?;
        Ok(Redirect::to("/admin#releases").into_response())
    })
    .await
}
async fn review_download(
    State(app): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
    request: Request,
) -> Result<Response> {
    let copy = app.clone();
    let path = blocking(move || {
        let conn = db(&copy)?;
        require_admin(&conn, &headers)?;
        let filename: Option<String> = conn
            .query_row(
                "SELECT r.filename FROM releases r JOIN release_reviews v ON v.release_id=r.id
                 WHERE r.id=?1 AND v.status!='rejected'",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(copy.config.data.join("uploads").join(
            filename.ok_or_else(|| Error(StatusCode::NOT_FOUND, "Release unavailable.".into()))?,
        ))
    })
    .await?;
    use tower::ServiceExt;
    let mut response = ServeFile::new(path)
        .oneshot(request)
        .await
        .map_err(internal)?
        .map(Body::new);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=review.zip"),
    );
    Ok(response)
}
#[derive(Deserialize)]
struct DeleteForm {
    csrf: String,
    confirm: String,
}
async fn delete_mod(
    State(app): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Timed(Form(form)): Timed<Form<DeleteForm>>,
) -> Result<Response> {
    csrf(&headers, &form.csrf)?;
    if form.confirm != "delete" {
        return Err(bad("Confirm removal by entering delete."));
    }
    blocking(move || {
        let mut conn = db(&app)?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let admin = require_admin(&tx, &headers)?;
        tx.execute(
            "INSERT OR IGNORE INTO file_deletions SELECT r.filename,m.owner_id,
                 r.size FROM releases r JOIN mods m ON m.id=r.mod_id WHERE m.id=?1",
            [&id],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO file_deletions SELECT id||'.zip',owner_id,size
                 FROM uploads WHERE mod_id=?1",
            [&id],
        )?;
        tx.execute("DELETE FROM uploads WHERE mod_id=?1", [&id])?;
        if tx.execute("DELETE FROM mods WHERE id=?1", [&id])? == 0 {
            return Err(Error(StatusCode::NOT_FOUND, "Mod not found.".into()));
        }
        audit(&tx, admin.id, "delete-mod", &id, "")?;
        tx.commit()?;
        collect_files(&app.config, &conn, None)?;
        Ok(Redirect::to("/admin#mods").into_response())
    })
    .await
}
