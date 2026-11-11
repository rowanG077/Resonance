use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use axum::{
    Form, Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, FromRequest, FromRequestParts, Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use minijinja::{Environment, context};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Seek},
    net::IpAddr,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tower_http::{limit::RequestBodyLimitLayer, services::ServeFile, trace::TraceLayer};

use crate::archive::validate as validate_archive;
use crate::error::{Error, Result, bad, internal};
use crate::model::UploadLimits;
use crate::model::{CATEGORIES, PART_BYTES, REPO, SESSION_AGE, hash, valid_token};
#[path = "moderation.rs"]
mod moderation;
#[path = "releases.rs"]
mod releases;
#[path = "uploads.rs"]
mod uploads;

#[derive(Clone)]
pub struct Config {
    pub data: PathBuf,
    pub public_url: String,
    pub registration_open: bool,
    pub disk_reserve_bytes: u64,
    pub trusted_proxies: Vec<IpAddr>,
}
impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let public_url =
            std::env::var("RESONANCE_WEB_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into());
        anyhow::ensure!(
            (public_url.starts_with("http://") || public_url.starts_with("https://"))
                && !public_url.ends_with('/'),
            "RESONANCE_WEB_URL must be an absolute http(s) origin without a trailing slash"
        );
        let uri = reqwest::Url::parse(&public_url)?;
        anyhow::ensure!(
            uri.host_str().is_some()
                && uri.path() == "/"
                && uri.query().is_none()
                && uri.fragment().is_none()
                && uri.username().is_empty()
                && uri.password().is_none(),
            "RESONANCE_WEB_URL must contain only scheme and host (optional port)"
        );
        Ok(Self {
            data: std::env::var_os("RESONANCE_WEB_DATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data")),
            public_url,
            trusted_proxies: std::env::var("RESONANCE_WEB_TRUSTED_PROXIES")
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| value.parse::<IpAddr>().map(|ip| ip.to_canonical()))
                .collect::<std::result::Result<_, _>>()?,
            disk_reserve_bytes: std::env::var("RESONANCE_WEB_DISK_RESERVE_MIB")
                .unwrap_or_else(|_| "2048".into())
                .parse::<u64>()?
                .checked_mul(crate::model::MIB)
                .ok_or_else(|| anyhow::anyhow!("Disk reserve is too large"))?,
            registration_open: std::env::var("RESONANCE_WEB_REGISTRATION")
                .unwrap_or_else(|_| "false".into())
                .parse()?,
        })
    }
}
struct App {
    config: Config,
    templates: Environment<'static>,
    mutation_slots: Arc<tokio::sync::Semaphore>,
    upload_slots: Arc<tokio::sync::Semaphore>,
    releases: releases::ReleaseCache,
}
type Shared = Arc<App>;
#[derive(Clone)]
struct ErrorMessage(String);
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let mut env = Environment::new();
        env.add_template("error.html", include_str!("../templates/error.html"))
            .unwrap();
        let message = self.1.clone();
        let mut response = (
            self.0,
            Html(
                env.get_template("error.html")
                    .unwrap()
                    .render(context! { message => self.1, status => self.0.as_u16() })
                    .unwrap(),
            ),
        )
            .into_response();
        response.extensions_mut().insert(ErrorMessage(message));
        response
    }
}
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(f).await.map_err(internal)?
}
// Bound the entire body read, including clients that keep sending tiny chunks.
// Only extraction is timed: a committed database operation must still finish.
struct Timed<T>(T);
impl<S, T> FromRequest<S> for Timed<T>
where
    S: Send + Sync,
    T: FromRequest<S>,
{
    type Rejection = Response;

    async fn from_request(request: Request, state: &S) -> std::result::Result<Self, Response> {
        tokio::time::timeout(
            std::time::Duration::from_secs(15),
            T::from_request(request, state),
        )
        .await
        .map_err(|_| {
            Error(
                StatusCode::REQUEST_TIMEOUT,
                "Request body timed out. Please retry.".into(),
            )
            .into_response()
        })?
        .map(Self)
        .map_err(IntoResponse::into_response)
    }
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
fn random_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("Operating-system randomness unavailable");
    hex::encode(bytes)
}
fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|h| h.to_str().ok())
        .flat_map(|h| h.split(';'))
        .find_map(|part| {
            let (key, value) = part.trim().split_once('=')?;
            (key == name).then(|| value.to_owned())
        })
}
fn csrf(headers: &HeaderMap, submitted: &str) -> Result<()> {
    let expected = cookie(headers, "resonance_csrf").unwrap_or_default();
    if !valid_token(&expected) || submitted != expected {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "This form expired. Go back, refresh the page and try again.".into(),
        ));
    }
    Ok(())
}
fn set_cookie(app: &App, response: &mut Response, name: &str, value: &str, age: i64) {
    let secure = if app.config.public_url.starts_with("https://") {
        "; Secure"
    } else {
        ""
    };
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "{name}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={age}{secure}"
        ))
        .unwrap(),
    );
}
fn db(app: &App) -> Result<Connection> {
    open_db(&app.config)
}
fn open_db(config: &Config) -> Result<Connection> {
    let conn = Connection::open(config.data.join("website.sqlite3"))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.pragma_update(None, "foreign_keys", true)?;
    Ok(conn)
}
#[derive(Serialize, Clone)]
struct User {
    id: i64,
    username: String,
    is_admin: bool,
}
fn user(conn: &Connection, headers: &HeaderMap) -> Result<Option<User>> {
    let token = cookie(headers, "resonance_session").unwrap_or_default();
    if !valid_token(&token) {
        return Ok(None);
    }
    Ok(conn.query_row("SELECT users.id, username, EXISTS(SELECT 1 FROM site_admins WHERE user_id = users.id) FROM sessions JOIN users ON users.id = sessions.user_id WHERE token_hash = ?1 AND expires_at > ?2", params![hash(token), now()], |r| Ok(User { id: r.get(0)?, username: r.get(1)?, is_admin: r.get(2)? })).optional()?)
}
fn require_user(conn: &Connection, headers: &HeaderMap) -> Result<User> {
    user(conn, headers)?.ok_or_else(|| {
        Error(
            StatusCode::UNAUTHORIZED,
            "Sign in to upload and manage your mods.".into(),
        )
    })
}
fn require_admin(conn: &Connection, headers: &HeaderMap) -> Result<User> {
    let user = require_user(conn, headers)?;
    if !user.is_admin {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "Administrator access is required.".into(),
        ));
    }
    Ok(user)
}
fn render(
    app: &App,
    conn: &Connection,
    headers: &HeaderMap,
    current_user: Option<&User>,
    template: &str,
    values: serde_json::Value,
) -> Result<Response> {
    let author_policy = current_user
        .map(|u| moderation::policy(conn, u.id))
        .transpose()?;
    let upload_limits = author_policy.as_ref().map(|p| p.limits).unwrap_or_default();
    let html = app.templates.get_template(template).map_err(internal)?.render(context! {
        user => current_user, upload_limits => upload_limits, csrf => cookie(headers, "resonance_csrf").unwrap_or_default(),
        registration_open => app.config.registration_open,
        author_policy => author_policy,
        repo => REPO, categories => CATEGORIES, data => values,
    }).map_err(internal)?;
    Ok(Html(html).into_response())
}

fn initialize(config: &Config) -> anyhow::Result<Connection> {
    std::fs::create_dir_all(config.data.join("uploads"))?;
    let conn = open_db(config)?;
    conn.execute_batch(include_str!("../schema.sql"))?;
    // Existing releases also require review when moderation is first enabled.
    conn.execute("INSERT OR IGNORE INTO release_reviews (release_id,metadata) SELECT r.id,json_object('title',m.title,'summary',m.summary,'description',m.description,'category',m.category) FROM releases r JOIN mods m ON m.id=r.mod_id", [])?;
    Ok(conn)
}

pub fn set_admin(config: &Config, username: &str, grant: bool) -> anyhow::Result<()> {
    let mut conn = initialize(config)?;
    let id: Option<i64> = conn
        .query_row(
            "SELECT id FROM users WHERE username = ?1",
            [username],
            |r| r.get(0),
        )
        .optional()?;
    let id = id.ok_or_else(|| {
        anyhow::anyhow!("Account {username:?} does not exist. Register it on the website first.")
    })?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    moderation::change_role(&tx, id, grant).map_err(|e| anyhow::anyhow!(e.1))?;
    tx.commit()?;
    Ok(())
}
pub fn list_accounts(config: &Config) -> anyhow::Result<Vec<(String, bool)>> {
    let conn = Connection::open_with_flags(
        config.data.join("website.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let mut stmt=conn.prepare("SELECT username,EXISTS(SELECT 1 FROM site_admins a WHERE a.user_id=u.id) FROM users u ORDER BY username")?;
    Ok(stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}
pub fn create_admin(config: &Config, username: &str, password: &str) -> anyhow::Result<()> {
    let username = validate_credentials(username, password).map_err(|e| anyhow::anyhow!(e.1))?;
    let salt = SaltString::encode_b64(&hex::decode(random_token()).unwrap())
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let password_hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .to_string();
    let mut conn = initialize(config)?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    anyhow::ensure!(
        !tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM users WHERE username=?1)",
            [&username],
            |r| r.get::<_, bool>(0)
        )?,
        "That account already exists. Use grant-admin to promote it."
    );
    tx.execute(
        "INSERT INTO users (username,password_hash) VALUES (?1,?2)",
        params![username, password_hash],
    )?;
    let id = tx.last_insert_rowid();
    tx.execute("INSERT INTO site_admins VALUES (?1)", [id])?;
    tx.commit()?;
    Ok(())
}

pub fn router(config: Config) -> anyhow::Result<Router> {
    router_with_releases(config, releases::ReleaseCache::new()?)
}
pub async fn maintain(config: Config) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
    loop {
        interval.tick().await;
        let config = config.clone();
        let result = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let conn = open_db(&config)?;
            uploads::cleanup(&config, &conn).map_err(|e| anyhow::anyhow!(e.1))?;
            Ok(())
        })
        .await;
        if !matches!(&result, Ok(Ok(()))) {
            tracing::error!(?result, "Upload cleanup failed");
        }
    }
}
fn router_with_releases(
    config: Config,
    releases: releases::ReleaseCache,
) -> anyhow::Result<Router> {
    let conn = initialize(&config)?;
    // A validation interrupted by a server restart can be retried from the browser.
    conn.execute(
        "UPDATE uploads SET status = 'uploading' WHERE status = 'validating'",
        [],
    )?;
    uploads::cleanup(&config, &conn).map_err(|e| anyhow::anyhow!(e.1))?;
    let mut templates = Environment::new();
    for (name, source) in [
        ("base.html", include_str!("../templates/base.html")),
        ("admin.html", include_str!("../templates/admin.html")),
        ("home.html", include_str!("../templates/home.html")),
        ("mods.html", include_str!("../templates/mods.html")),
        ("mod.html", include_str!("../templates/mod.html")),
        ("auth.html", include_str!("../templates/auth.html")),
        ("upload.html", include_str!("../templates/upload.html")),
        (
            "dashboard.html",
            include_str!("../templates/dashboard.html"),
        ),
        ("cards.html", include_str!("../templates/cards.html")),
    ] {
        templates.add_template(name, source)?;
    }
    let app = Arc::new(App {
        config,
        templates,
        mutation_slots: Arc::new(tokio::sync::Semaphore::new(8)),
        upload_slots: Arc::new(tokio::sync::Semaphore::new(2)),
        releases,
    });
    Ok(Router::new()
        .route("/", get(home))
        .route("/download/{platform}", get(releases::download))
        .route("/healthz", get(|| async { "ok" }))
        .route("/mods", get(browse))
        .route("/mods/new", get(upload_page))
        .merge(uploads::routes())
        .merge(moderation::routes())
        .route("/mods/{id}", get(mod_page))
        .route("/mods/{id}/visibility", post(visibility))
        .route("/account/{mode}", get(auth_page).post(auth))
        .route("/account/logout", post(logout))
        .route("/dashboard", get(dashboard))
        .route("/api/v1/mods", get(manifest))
        .route("/api/v1/mods/{id}", get(mod_manifest))
        .route("/api/v1/releases/{id}/download", get(download))
        .route("/manifest.json", get(manifest))
        .route(
            "/static/style.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    include_str!("../static/style.css"),
                )
            }),
        )
        .route(
            "/static/menu-pattern.svg",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "image/svg+xml")],
                    include_str!("../static/menu-pattern.svg"),
                )
            }),
        )
        .route(
            "/static/kalam-regular.woff2",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "font/woff2")],
                    include_bytes!("../static/kalam-regular.woff2").as_slice(),
                )
            }),
        )
        .route(
            "/static/upload.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../static/upload.js"),
                )
            }),
        )
        .route(
            "/static/window-frame.svg",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "image/svg+xml")],
                    include_str!("../static/window-frame.svg"),
                )
            }),
        )
        .route(
            "/static/resonance.svg",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "image/svg+xml")],
                    include_bytes!("../../../resources/logo.svg").as_slice(),
                )
            }),
        )
        .fallback(|| async {
            Error(
                StatusCode::NOT_FOUND,
                "That page could not be found.".into(),
            )
        })
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(RequestBodyLimitLayer::new(PART_BYTES as usize))
        .layer(middleware::from_fn_with_state(app.clone(), limit_mutations))
        .layer(middleware::from_fn_with_state(app.clone(), security))
        .layer(TraceLayer::new_for_http())
        .with_state(app))
}
async fn limit_mutations(State(app): State<Shared>, request: Request, next: Next) -> Response {
    // Slow request bodies must not consume the capacity for public pages or
    // downloads. Reject excess mutations immediately instead of queuing them.
    if request.method().is_safe() {
        return next.run(request).await;
    }
    let Ok(permit) = app.mutation_slots.clone().try_acquire_owned() else {
        return Error(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many account or upload requests. Please retry shortly.".into(),
        )
        .into_response();
    };
    // Finish admitted mutations even if the caller disconnects. The permit
    // must outlive their non-cancellable database and password work.
    tokio::spawn(async move {
        let _permit = permit;
        next.run(request).await
    })
    .await
    .unwrap_or_else(|error| internal(error).into_response())
}
async fn security(State(app): State<Shared>, mut request: Request, next: Next) -> Response {
    let api = request.uri().path().starts_with("/api/") || request.uri().path() == "/manifest.json";
    let token = cookie(request.headers(), "resonance_csrf").filter(|v| valid_token(v));
    let fresh = token.is_none();
    let token = token.unwrap_or_else(random_token);
    if fresh {
        let old = request
            .headers()
            .get(header::COOKIE)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("");
        let value = format!("resonance_csrf={token}; {old}");
        request
            .headers_mut()
            .insert(header::COOKIE, value.parse().unwrap());
    }
    let mut response = next.run(request).await;
    if api && (response.status().is_client_error() || response.status().is_server_error()) {
        let message = response
            .extensions()
            .get::<ErrorMessage>()
            .map(|e| e.0.clone())
            .unwrap_or_else(|| {
                response
                    .status()
                    .canonical_reason()
                    .unwrap_or("Request failed")
                    .to_owned()
            });
        response = (
            response.status(),
            Json(serde_json::json!({"error":message})),
        )
            .into_response();
    }
    if fresh {
        set_cookie(&app, &mut response, "resonance_csrf", &token, SESSION_AGE);
    }
    let headers = response.headers_mut();
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        "referrer-policy",
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    headers.insert("content-security-policy", HeaderValue::from_static("default-src 'self'; style-src 'self'; img-src 'self'; script-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if app.config.public_url.starts_with("https://") {
        headers.insert(
            "strict-transport-security",
            HeaderValue::from_static("max-age=31536000"),
        );
    }
    response
}

struct ClientAddress(String);
impl FromRequestParts<Shared> for ClientAddress {
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, app: &Shared) -> Result<Self> {
        client_address(&app.config, parts).map(Self)
    }
}

fn client_address(config: &Config, request: &Parts) -> Result<String> {
    let Some(peer) = request
        .extensions
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|info| info.0.ip().to_canonical())
    else {
        return Ok("local".into());
    };
    if !config.trusted_proxies.contains(&peer) {
        return Ok(peer.to_string());
    }
    // The trusted proxy must overwrite this header with its resolved client IP.
    // A single address avoids interpreting proxy chains differently from Caddy.
    let mut values = request.headers.get_all("x-resonance-client-ip").iter();
    let client = values
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<IpAddr>().ok());
    match client {
        Some(client) if values.next().is_none() => Ok(client.to_canonical().to_string()),
        _ => Err(bad(
            "The trusted proxy must supply one X-Resonance-Client-IP address.",
        )),
    }
}

#[derive(Serialize)]
struct Release {
    id: String,
    version: String,
    resonance_version: String,
    content_version: u32,
    sha256: String,
    size: i64,
    created_at: String,
    download_url: String,
    review_status: String,
    review_note: String,
}
#[derive(Serialize)]
struct Mod {
    id: String,
    title: String,
    summary: String,
    description: String,
    category: String,
    author: String,
    published: bool,
    created_at: String,
    page_url: String,
    releases: Vec<Release>,
}
fn mods(conn: &Connection, app: &App, owner: Option<i64>, id: Option<&str>) -> Result<Vec<Mod>> {
    let mut stmt = conn.prepare(
        "SELECT m.id,m.title,m.summary,m.description,m.category,u.username,m.published,m.created_at
         FROM mods m JOIN users u ON u.id=m.owner_id
         WHERE ((?1 IS NULL AND EXISTS(SELECT 1 FROM public_releases WHERE mod_id=m.id))
                OR m.owner_id=?1)
           AND (?2 IS NULL OR m.id=?2)
         ORDER BY m.rowid DESC",
    )?;
    let mut items = stmt
        .query_map(params![owner, id], |r| {
            Ok(Mod {
                id: r.get(0)?,
                title: r.get(1)?,
                summary: r.get(2)?,
                description: r.get(3)?,
                category: r.get(4)?,
                author: r.get(5)?,
                published: r.get(6)?,
                created_at: r.get(7)?,
                page_url: String::new(),
                releases: vec![],
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for item in &mut items {
        item.page_url = format!("{}/mods/{}", app.config.public_url, item.id);
        let mut stmt = conn.prepare(
            "SELECT r.id,r.version,r.resonance_version,r.content_version,r.sha256,r.size,
                    r.created_at,v.status,v.note
             FROM releases r JOIN release_reviews v ON v.release_id=r.id
             WHERE r.mod_id=?1
               AND (?2 IS NOT NULL OR r.id IN (SELECT id FROM public_releases WHERE mod_id=?1))
             ORDER BY r.rowid DESC",
        )?;
        item.releases = stmt
            .query_map(params![item.id, owner], |r| {
                let id: String = r.get(0)?;
                Ok(Release {
                    download_url: format!(
                        "{}/api/v1/releases/{id}/download",
                        app.config.public_url
                    ),
                    id,
                    version: r.get(1)?,
                    resonance_version: r.get(2)?,
                    content_version: r.get(3)?,
                    sha256: r.get(4)?,
                    size: r.get(5)?,
                    created_at: r.get(6)?,
                    review_status: r.get(7)?,
                    review_note: if owner.is_some() {
                        r.get(8)?
                    } else {
                        String::new()
                    },
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
    }
    if owner.is_none() {
        items.retain(|m| !m.releases.is_empty());
    }
    Ok(items)
}
fn featured_mods(conn: &Connection) -> Result<Vec<serde_json::Value>> {
    let mut stmt = conn.prepare(
        "SELECT m.id,m.title,m.summary,m.category,u.username,r.version,r.size
         FROM mods m JOIN users u ON u.id=m.owner_id
         JOIN releases r ON r.id=(
             SELECT id FROM public_releases WHERE mod_id=m.id
             ORDER BY release_order DESC LIMIT 1
         )
         ORDER BY m.rowid DESC LIMIT 3",
    )?;
    Ok(stmt
        .query_map([], |r| {
            Ok(serde_json::json!({
                "id": r.get::<_, String>(0)?, "title": r.get::<_, String>(1)?,
                "summary": r.get::<_, String>(2)?, "category": r.get::<_, String>(3)?,
                "author": r.get::<_, String>(4)?,
                "releases": [{"version": r.get::<_, String>(5)?, "size": r.get::<_, i64>(6)?}],
            }))
        })?
        .collect::<std::result::Result<_, _>>()?)
}
async fn home(State(app): State<Shared>, headers: HeaderMap) -> Result<Response> {
    let release = app.releases.latest().await;
    blocking(move || {
        let conn = db(&app)?;
        let current_user = user(&conn, &headers)?;
        let items = featured_mods(&conn)?;
        render(
            &app,
            &conn,
            &headers,
            current_user.as_ref(),
            "home.html",
            serde_json::json!({"mods": items, "release": release}),
        )
    })
    .await
}
#[derive(Deserialize, Default)]
struct Filters {
    q: Option<String>,
    category: Option<String>,
    r#mod: Option<String>,
}
async fn browse(
    State(app): State<Shared>,
    headers: HeaderMap,
    Query(filters): Query<Filters>,
) -> Result<Response> {
    blocking(move || {
        let q = filters.q.unwrap_or_default();
        let category = filters.category.unwrap_or_default();
        let conn = db(&app)?;
        let current_user = user(&conn, &headers)?;
        let items: Vec<_> = mods(&conn, &app, None, None)?
            .into_iter()
            .filter(|m| {
                (category.is_empty() || category == m.category)
                    && format!("{} {} {}", m.title, m.summary, m.author)
                        .to_lowercase()
                        .contains(&q.to_lowercase())
            })
            .collect();
        render(
            &app,
            &conn,
            &headers,
            current_user.as_ref(),
            "mods.html",
            serde_json::json!({"mods": items, "q": q, "category": category}),
        )
    })
    .await
}
async fn mod_page(
    State(app): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response> {
    blocking(move || {
        let conn = db(&app)?;
        let current_user = user(&conn, &headers)?;
        let owner = current_user.as_ref().map(|u| u.id);
        let mut item = if let Some(owner) = owner {
            mods(&conn, &app, Some(owner), Some(&id))?
                .into_iter()
                .next()
        } else {
            None
        };
        if item.is_none() {
            item = mods(&conn, &app, None, Some(&id))?.into_iter().next();
        }
        let item =
            item.ok_or_else(|| Error(StatusCode::NOT_FOUND, "This mod is unavailable.".into()))?;
        let can_edit = owner.is_some_and(|uid| {
            conn.query_row("SELECT owner_id FROM mods WHERE id = ?1", [&id], |r| {
                r.get::<_, i64>(0)
            })
            .ok()
                == Some(uid)
        });
        render(
            &app,
            &conn,
            &headers,
            current_user.as_ref(),
            "mod.html",
            serde_json::json!({"mod": item, "can_edit": can_edit}),
        )
    })
    .await
}
async fn dashboard(State(app): State<Shared>, headers: HeaderMap) -> Result<Response> {
    blocking(move || {
        let conn = db(&app)?;
        let Some(owner) = user(&conn, &headers)? else {
            return Ok(Redirect::to("/account/login").into_response());
        };
        render(
            &app,
            &conn,
            &headers,
            Some(&owner),
            "dashboard.html",
            serde_json::json!({"mods": mods(&conn, &app, Some(owner.id), None)?}),
        )
    })
    .await
}
fn registration_allowed(app: &App, mode: &str) -> Result<()> {
    if mode == "register" && !app.config.registration_open {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "Account creation is currently closed.".into(),
        ));
    }
    Ok(())
}

async fn auth_page(
    State(app): State<Shared>,
    headers: HeaderMap,
    Path(mode): Path<String>,
) -> Result<Response> {
    registration_allowed(&app, &mode)?;
    if !["register", "login"].contains(&mode.as_str()) {
        return Err(Error(StatusCode::NOT_FOUND, "Page not found.".into()));
    }
    blocking(move || {
        let conn = db(&app)?;
        let current_user = user(&conn, &headers)?;
        render(
            &app,
            &conn,
            &headers,
            current_user.as_ref(),
            "auth.html",
            serde_json::json!({"mode": mode}),
        )
    })
    .await
}
#[derive(Deserialize)]
struct Credentials {
    username: String,
    password: String,
    csrf: String,
}
async fn auth(
    State(app): State<Shared>,
    ClientAddress(peer): ClientAddress,
    headers: HeaderMap,
    Path(mode): Path<String>,
    Timed(Form(form)): Timed<Form<Credentials>>,
) -> Result<Response> {
    csrf(&headers, &form.csrf)?;
    if !["register", "login"].contains(&mode.as_str()) {
        return Err(bad("Unknown account action."));
    }
    blocking(move || authenticate(&app, &headers, &peer, &mode, &form)).await
}

fn authenticate(
    app: &App,
    headers: &HeaderMap,
    peer: &str,
    mode: &str,
    form: &Credentials,
) -> Result<Response> {
    registration_allowed(app, mode)?;
    let mut conn = db(app)?;
    throttle_auth(&mut conn, peer, &form.username)?;
    let username = validate_credentials(&form.username, &form.password)?;
    let uid = if mode == "register" {
        let salt =
            SaltString::encode_b64(&hex::decode(random_token()).unwrap()).map_err(internal)?;
        let password_hash = Argon2::default()
            .hash_password(form.password.as_bytes(), &salt)
            .map_err(internal)?
            .to_string();
        match conn.execute(
            "INSERT INTO users (username, password_hash) VALUES (?1, ?2)",
            params![username, password_hash],
        ) {
            Ok(_) => conn.last_insert_rowid(),
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                return Err(bad("That username is already taken."));
            }
            Err(e) => return Err(e.into()),
        }
    } else {
        let row: Option<(i64, String)> = conn
            .query_row(
                "SELECT id, password_hash FROM users WHERE username = ?1",
                [&username],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        // Perform the same expensive verification for an unknown account.
        let dummy = "$argon2id$v=19$m=19456,t=2,p=1$MTIzNDU2Nzg5MDEyMzQ1Ng$YWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWE";
        let encoded = row.as_ref().map(|r| r.1.as_str()).unwrap_or(dummy);
        let verified = Argon2::default()
            .verify_password(
                form.password.as_bytes(),
                &PasswordHash::new(encoded).map_err(internal)?,
            )
            .is_ok();
        match row {
            Some((id, _)) if verified => id,
            _ => {
                return Err(Error(
                    StatusCode::UNAUTHORIZED,
                    "Username or password is incorrect.".into(),
                ));
            }
        }
    };
    conn.execute("DELETE FROM sessions WHERE expires_at < ?1", [now()])?;
    if let Some(old) = cookie(headers, "resonance_session") {
        conn.execute("DELETE FROM sessions WHERE token_hash = ?1", [hash(old)])?;
    }
    let token = random_token();
    conn.execute(
        "INSERT INTO sessions VALUES (?1, ?2, ?3)",
        params![hash(&token), uid, now() + SESSION_AGE],
    )?;
    let mut response = Redirect::to("/dashboard").into_response();
    set_cookie(app, &mut response, "resonance_session", &token, SESSION_AGE);
    set_cookie(
        app,
        &mut response,
        "resonance_csrf",
        &random_token(),
        SESSION_AGE,
    );
    Ok(response)
}
fn throttle_auth(conn: &mut Connection, peer: &str, username: &str) -> Result<()> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let time = now();
    tx.execute("DELETE FROM auth_attempts WHERE reset_at <= ?1", [time])?;
    // Charge the client even when its account bucket is exhausted. Rotating
    // usernames cannot evade the client limit or create unbounded account rows.
    for (key, maximum) in [
        (format!("client:{peer}"), 60),
        (
            format!("account:{peer}:{}", username.trim().to_lowercase()),
            15,
        ),
    ] {
        let address = hash(key);
        tx.execute("INSERT INTO auth_attempts VALUES (?1, 1, ?2) ON CONFLICT(address) DO UPDATE SET attempts = MIN(attempts + 1, ?3)", params![address, time + 900, maximum + 1])?;
        let attempts: i64 = tx.query_row(
            "SELECT attempts FROM auth_attempts WHERE address = ?1",
            [&address],
            |r| r.get(0),
        )?;
        if attempts > maximum {
            tx.commit()?;
            return Err(Error(
                StatusCode::TOO_MANY_REQUESTS,
                "Too many attempts. Try again in 15 minutes.".into(),
            ));
        }
    }
    tx.commit()?;
    Ok(())
}
fn validate_credentials(username: &str, password: &str) -> Result<String> {
    let username = username.trim().to_lowercase();
    if !(3..=24).contains(&username.len())
        || !username
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    {
        return Err(bad(
            "Use 3–24 letters, numbers, underscores or hyphens for your username.",
        ));
    }
    if !(10..=128).contains(&password.len()) {
        return Err(bad("Use a password of 10–128 bytes."));
    }
    Ok(username)
}
#[derive(Deserialize)]
struct CsrfForm {
    csrf: String,
}
async fn logout(
    State(app): State<Shared>,
    headers: HeaderMap,
    Timed(Form(form)): Timed<Form<CsrfForm>>,
) -> Result<Response> {
    csrf(&headers, &form.csrf)?;
    blocking(move || {
        if let Some(token) = cookie(&headers, "resonance_session") {
            db(&app)?.execute("DELETE FROM sessions WHERE token_hash = ?1", [hash(token)])?;
        }
        let mut response = Redirect::to("/").into_response();
        set_cookie(&app, &mut response, "resonance_session", "", 0);
        Ok(response)
    })
    .await
}
async fn upload_page(
    State(app): State<Shared>,
    headers: HeaderMap,
    Query(filters): Query<Filters>,
) -> Result<Response> {
    blocking(move || {
        let conn = db(&app)?;
        let Some(owner) = user(&conn, &headers)? else {
            return Ok(Redirect::to("/account/login").into_response());
        };
        // Over-quota authors still need this page to cancel their reservation.
        moderation::upload_access(&conn, owner.id)?;
        let item = if let Some(id) = filters.r#mod {
            Some(
                mods(&conn, &app, Some(owner.id), Some(&id))?
                    .into_iter()
                    .next()
                    .ok_or_else(|| {
                        Error(
                            StatusCode::NOT_FOUND,
                            "Mod not found in your account.".into(),
                        )
                    })?,
            )
        } else {
            None
        };
        render(
            &app,
            &conn,
            &headers,
            Some(&owner),
            "upload.html",
            serde_json::json!({"mod": item}),
        )
    })
    .await
}
#[derive(Deserialize)]
struct Visibility {
    csrf: String,
    published: bool,
}
async fn visibility(
    State(app): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Timed(Form(form)): Timed<Form<Visibility>>,
) -> Result<Response> {
    csrf(&headers, &form.csrf)?;
    blocking(move || {
        let conn = db(&app)?;
        let owner = require_user(&conn, &headers)?;
        let count = conn.execute(
            "UPDATE mods SET published = ?1 WHERE id = ?2 AND owner_id = ?3",
            params![form.published, id, owner.id],
        )?;
        if count != 1 {
            return Err(Error(
                StatusCode::NOT_FOUND,
                "Mod not found in your account.".into(),
            ));
        }
        Ok(Redirect::to("/dashboard").into_response())
    })
    .await
}
async fn manifest(State(app): State<Shared>) -> Result<Response> {
    blocking(move || {
        Ok(Json(
            serde_json::json!({"schema_version": 1, "mods": mods(&db(&app)?, &app, None, None)?}),
        )
        .into_response())
    })
    .await
}
async fn mod_manifest(State(app): State<Shared>, Path(id): Path<String>) -> Result<Response> {
    blocking(move || {
        let item = mods(&db(&app)?, &app, None, Some(&id))?
            .into_iter()
            .next()
            .ok_or_else(|| Error(StatusCode::NOT_FOUND, "Mod not found.".into()))?;
        Ok(Json(serde_json::json!({"schema_version": 1, "mod": item})).into_response())
    })
    .await
}
async fn download(
    State(app): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Result<Response> {
    let (path, name) = blocking(move || {
        let conn = db(&app)?;
        let row: Option<(String, String, String)> = conn
            .query_row(
                "SELECT filename,mod_id,version FROM public_releases WHERE id=?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let (filename, mod_id, version) = row.ok_or_else(|| {
            Error(
                StatusCode::NOT_FOUND,
                "This download is unavailable.".into(),
            )
        })?;
        Ok((
            app.config.data.join("uploads").join(filename),
            format!("{mod_id}-{version}.zip"),
        ))
    })
    .await?;
    // Stream archives rather than holding a second full copy in memory.
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
        format!("attachment; filename=\"{name}\"")
            .parse()
            .map_err(internal)?,
    );
    Ok(response)
}

#[cfg(test)]
#[path = "web_tests.rs"]
mod tests;
