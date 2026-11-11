use super::*;
use crate::model::MIB;
use http_body_util::BodyExt;
use serde_json::Value;
use serde_json::json;
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
};
use tower::ServiceExt;

struct Browser {
    app: Router,
    cookies: BTreeMap<String, String>,
    peer: std::net::SocketAddr,
    client_ip: Option<String>,
}
impl Browser {
    fn new(app: Router) -> Self {
        Self {
            app,
            cookies: BTreeMap::new(),
            peer: "127.0.0.1:1234".parse().unwrap(),
            client_ip: None,
        }
    }
    async fn request(
        &mut self,
        method: &str,
        path: &str,
        content_type: &str,
        body: Vec<u8>,
    ) -> (StatusCode, HeaderMap, Vec<u8>) {
        self.request_body(method, path, content_type, Body::from(body))
            .await
    }
    async fn request_body(
        &mut self,
        method: &str,
        path: &str,
        content_type: &str,
        body: Body,
    ) -> (StatusCode, HeaderMap, Vec<u8>) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::CONTENT_TYPE, content_type)
            .header(
                "x-csrf-token",
                self.cookies
                    .get("resonance_csrf")
                    .cloned()
                    .unwrap_or_default(),
            )
            .header(
                header::COOKIE,
                self.cookies
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join("; "),
            )
            .body(body)
            .unwrap();
        request
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(self.peer));
        if let Some(client_ip) = &self.client_ip {
            request
                .headers_mut()
                .insert("x-resonance-client-ip", client_ip.parse().unwrap());
        }
        let response = self.app.clone().oneshot(request).await.unwrap();
        for value in response.headers().get_all(header::SET_COOKIE) {
            let (name, value) = value
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .split_once('=')
                .unwrap();
            self.cookies.insert(name.into(), value.into());
        }
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec();
        (status, headers, bytes)
    }
    async fn get(&mut self, path: &str) -> (StatusCode, HeaderMap, Vec<u8>) {
        self.request("GET", path, "", vec![]).await
    }
    async fn post(
        &mut self,
        path: &str,
        fields: &[(&str, &str)],
    ) -> (StatusCode, HeaderMap, Vec<u8>) {
        let csrf = self
            .cookies
            .get("resonance_csrf")
            .cloned()
            .unwrap_or_default();
        let mut form = fields.to_vec();
        form.push(("csrf", &csrf));
        self.request(
            "POST",
            path,
            "application/x-www-form-urlencoded",
            serde_urlencoded::to_string(form).unwrap().into_bytes(),
        )
        .await
    }
    async fn policy(
        &mut self,
        config: &Config,
        name: &str,
        changes: &[(&str, &str)],
    ) -> (StatusCode, HeaderMap, Vec<u8>) {
        let id = account_id(config, name);
        let current = moderation::policy(&open_db(config).unwrap(), id).unwrap();
        let mut fields = BTreeMap::from([
            ("status", current.status),
            ("quota_mib", current.quota_mib.to_string()),
            ("daily_mib", current.daily_mib.to_string()),
            ("zip_mib", current.limits.zip_mib.to_string()),
            ("expanded_mib", current.limits.expanded_mib.to_string()),
        ]);
        fields.extend(changes.iter().map(|(k, v)| (*k, v.to_string())));
        let fields: Vec<_> = fields.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.post(&format!("/admin/accounts/{id}"), &fields).await
    }
    async fn register(&mut self, username: &str) {
        assert_eq!(self.get("/account/register").await.0, StatusCode::OK);
        let (status, _, body) = self
            .post(
                "/account/register",
                &[
                    ("username", username),
                    ("password", "correct horse battery staple"),
                ],
            )
            .await;
        assert_eq!(
            status,
            StatusCode::SEE_OTHER,
            "{}",
            String::from_utf8_lossy(&body)
        );
    }
    async fn start_upload(
        &mut self,
        mod_id: &str,
        version: &str,
        size: u64,
        fingerprint: String,
    ) -> (StatusCode, HeaderMap, Vec<u8>) {
        let metadata = json!({
            "mod_id": if mod_id.is_empty() {None} else {Some(mod_id)},
            "title":"Moonlit battle", "summary":"A fresh look for the battle interface.",
            "description":"An original UI mod. <script>alert('unsafe')</script> is plain text.",
            "category":"Visuals", "version":version, "resonance_version":"0.4.0", "content_version":102, "rights":true
        });
        self.request(
            "POST",
            "/api/v1/uploads",
            "application/json",
            serde_json::to_vec(&json!({"metadata":metadata,"size":size,"fingerprint":fingerprint}))
                .unwrap(),
        )
        .await
    }
    async fn finish(&mut self, id: &str) -> (StatusCode, HeaderMap, Vec<u8>) {
        let response = self
            .post(&format!("/api/v1/uploads/{id}/complete"), &[])
            .await;
        if response.0 != StatusCode::OK {
            return response;
        }
        for _ in 0..1000 {
            let response = self.get(&format!("/api/v1/uploads/{id}")).await;
            match json(&response.2)["status"].as_str().unwrap() {
                "submitted" => return response,
                "failed" => return (StatusCode::BAD_REQUEST, response.1, response.2),
                _ => tokio::time::sleep(std::time::Duration::from_millis(10)).await,
            }
        }
        panic!("Upload did not finish");
    }
    async fn upload(
        &mut self,
        mod_id: &str,
        version: &str,
        archive: &[u8],
    ) -> (StatusCode, HeaderMap, Vec<u8>) {
        let parts: Vec<_> = archive.chunks(PART_BYTES as usize).collect();
        let fingerprint = format!("{}{}", hash(parts[0]), hash(parts.last().unwrap()));
        let response = self
            .start_upload(mod_id, version, archive.len() as u64, fingerprint)
            .await;
        if response.0 != StatusCode::OK {
            return response;
        }
        let id = json(&response.2)["id"].as_str().unwrap().to_owned();
        for (i, part) in parts.into_iter().enumerate() {
            let response = self
                .request(
                    "PUT",
                    &format!("/api/v1/uploads/{id}/parts/{}", i + 1),
                    "application/octet-stream",
                    part.to_vec(),
                )
                .await;
            if response.0 != StatusCode::OK {
                return response;
            }
        }
        self.finish(&id).await
    }
}
fn setup() -> (tempfile::TempDir, Config, Router) {
    let data = tempfile::tempdir().unwrap();
    let config = Config {
        data: data.path().to_owned(),
        public_url: "https://mods.example.test".into(),
        registration_open: true,
        disk_reserve_bytes: 0,
        trusted_proxies: vec![],
    };
    let app = router_with_releases(config.clone(), releases::ReleaseCache::offline()).unwrap();
    (data, config, app)
}
fn archive(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        writer
            .start_file(
                *name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}
fn json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap()
}
fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn cancelled_completion_still_submits_the_release() {
    // Hold the sole blocking worker so cancellation happens deterministically
    // after completion is queued, before its database transition runs.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    runtime.block_on(async {
        let (_data, config, app) = setup();
        let mut owner = Browser::new(app);
        owner.register("owner").await;
        approve_uploader(&config, "owner");
        let bytes = archive(&[("ui/settings.json", b"{}")]);
        let started = owner
            .start_upload(
                "",
                "1.0.0",
                bytes.len() as u64,
                format!("{}{}", hash(&bytes), hash(&bytes)),
            )
            .await;
        assert_eq!(started.0, StatusCode::OK);
        let id = json(&started.2)["id"].as_str().unwrap().to_owned();
        assert_eq!(
            owner
                .request("PUT", &format!("/api/v1/uploads/{id}/parts/1"), "", bytes)
                .await
                .0,
            StatusCode::OK
        );

        let (release_worker, wait) = std::sync::mpsc::channel();
        let (started, ready) = tokio::sync::oneshot::channel();
        let worker = tokio::task::spawn_blocking(move || {
            started.send(()).unwrap();
            wait.recv().unwrap();
        });
        ready.await.unwrap();
        let path = format!("/api/v1/uploads/{id}/complete");
        let mut completion = Box::pin(owner.post(&path, &[]));
        std::future::poll_fn(|cx| {
            assert!(completion.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        drop(completion);
        release_worker.send(()).unwrap();
        worker.await.unwrap();

        // Poll only status: a second completion request must not be needed.
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let response = owner.get(&format!("/api/v1/uploads/{id}")).await;
                assert_eq!(response.0, StatusCode::OK);
                match json(&response.2)["status"].as_str().unwrap() {
                    "submitted" => break,
                    "uploading" | "validating" => {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await
                    }
                    status => panic!("Unexpected upload status: {status}"),
                }
            }
        })
        .await
        .expect("Cancelled completion left the upload without a validation worker");
    });
}

#[test]
fn homepage_selects_three_visible_mods_with_their_latest_approved_release() {
    let (_data, config, _app) = setup();
    let conn = initialize(&config).unwrap();
    conn.execute_batch(
        "INSERT INTO users (id,username,password_hash) VALUES (1,'owner','unused'),(2,'suspended','unused');
         INSERT INTO author_policy (user_id,status) VALUES (2,'suspended');",
    )
    .unwrap();
    for (id, owner, published, review) in [
        ("oldest", 1, true, "approved"),
        ("third", 1, true, "approved"),
        ("second", 1, true, "approved"),
        ("first", 1, true, "approved"),
        ("hidden", 1, false, "approved"),
        ("suspended", 2, true, "approved"),
        ("pending", 1, true, "pending"),
        ("rejected", 1, true, "rejected"),
        ("empty", 1, true, ""),
    ] {
        conn.execute(
            "INSERT INTO mods (id,owner_id,title,summary,description,category,published)
             VALUES (?1,?2,?1,'Summary','Description','Visuals',?3)",
            params![id, owner, published],
        )
        .unwrap();
        if review.is_empty() {
            continue;
        }
        for (version, status) in [("1.0.0", review), ("2.0.0", review), ("3.0.0", "pending")] {
            let release = format!("{id}-{version}");
            conn.execute(
                "INSERT INTO releases (id,mod_id,version,resonance_version,content_version,filename,sha256,size)
                 VALUES (?1,?2,?3,'0.4.0',1,?1,'unused',1024)",
                params![release, id, version],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO release_reviews (release_id,status,metadata) VALUES (?1,?2,'{}')",
                params![release, status],
            )
            .unwrap();
        }
    }
    let cards = featured_mods(&conn).unwrap();
    assert_eq!(
        cards
            .iter()
            .map(|card| card["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["first", "second", "third"]
    );
    for card in cards {
        assert_eq!(card["author"], "owner");
        assert_eq!(card["releases"], json!([{"version":"2.0.0","size":1024}]));
    }
}

#[tokio::test]
async fn complete_mod_lifecycle_with_persistence_and_owner_isolation() {
    let (_data, config, app) = setup();
    let mut alice = Browser::new(app.clone());
    let mut public = Browser::new(app.clone());
    for path in [
        "/",
        "/mods",
        "/account/login",
        "/account/register",
        "/static/style.css",
        "/static/resonance.svg",
        "/static/menu-pattern.svg",
        "/static/window-frame.svg",
        "/api/v1/mods",
        "/manifest.json",
    ] {
        assert_eq!(public.get(path).await.0, StatusCode::OK, "{path}");
    }
    assert!(text(&public.get("/").await.2).contains("Resonance/releases/latest"));
    assert_eq!(public.get("/dashboard").await.0, StatusCode::SEE_OTHER);
    assert_eq!(public.get("/mods/new").await.0, StatusCode::SEE_OTHER);
    assert_eq!(
        public.upload("", "1.0.0", b"bad zip").await.0,
        StatusCode::UNAUTHORIZED
    );
    alice.register("Alice").await;
    approve_uploader(&config, "Alice");
    assert_eq!(alice.get("/mods/new").await.0, StatusCode::OK);
    assert_eq!(
        alice.upload("", "1.0.0", b"bad zip").await.0,
        StatusCode::BAD_REQUEST
    );
    assert!(
        json(&public.get("/api/v1/mods").await.2)["mods"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let larger_asset = vec![b' '; 20 * 1024];
    let zip = archive(&[
        ("ui/custom.json", br#"{"original":true}"#),
        ("scripts/additional.sym", &larger_asset),
        ("README.md", b"My original mod"),
    ]);
    let (status, _, body) = alice.upload("", "1.0.0", &zip).await;
    assert_eq!(status, StatusCode::OK, "{}", text(&body));
    approve_releases(&config);
    let page = json(&body)["page_url"].as_str().unwrap().to_owned();
    let id = page.trim_start_matches("/mods/");
    assert_eq!(alice.get("/dashboard").await.0, StatusCode::OK);
    assert_eq!(
        alice.get(&format!("/mods/new?mod={id}")).await.0,
        StatusCode::OK
    );
    let (_, _, html) = public.get(&page).await;
    assert!(text(&html).contains("&lt;script&gt;"));
    assert!(!text(&html).contains("<script>"));
    assert!(
        text(&public.get("/mods?q=moonlit&category=Visuals").await.2).contains("Moonlit battle")
    );
    assert!(text(&public.get("/mods?q=missing").await.2).contains("No mods found"));
    let catalogue = json(&public.get("/api/v1/mods").await.2);
    assert_eq!(catalogue["schema_version"], 1);
    assert_eq!(catalogue["mods"].as_array().unwrap().len(), 1);
    let release = &catalogue["mods"][0]["releases"][0];
    assert_eq!(release["sha256"], hash(&zip));
    assert_eq!(release["size"], zip.len());
    assert_eq!(release["content_version"], 102);
    let url = release["download_url"]
        .as_str()
        .unwrap()
        .strip_prefix("https://mods.example.test")
        .unwrap();
    let (status, headers, downloaded) = public.get(url).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "application/zip");
    assert_eq!(downloaded, zip);
    assert_eq!(
        public.get(&format!("/api/v1/mods/{id}")).await.0,
        StatusCode::OK
    );
    assert_eq!(
        alice.upload(id, "1.0.0", &zip).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        alice.upload(id, "0.9.0", &zip).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        alice.upload(id, "1.0.0+repacked", &zip).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(alice.upload(id, "1.1.0", &zip).await.0, StatusCode::OK);
    approve_releases(&config);
    let mut bob = Browser::new(app.clone());
    bob.register("Bob").await;
    approve_uploader(&config, "Bob");
    assert_eq!(bob.upload(id, "2.0.0", &zip).await.0, StatusCode::FORBIDDEN);
    assert_eq!(
        bob.post(&format!("/mods/{id}/visibility"), &[("published", "false")])
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        alice
            .post(&format!("/mods/{id}/visibility"), &[("published", "false")])
            .await
            .0,
        StatusCode::SEE_OTHER
    );
    assert_eq!(public.get(url).await.0, StatusCode::NOT_FOUND);
    assert_eq!(public.get(&page).await.0, StatusCode::NOT_FOUND);
    assert_eq!(bob.get(&page).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        public.get(&format!("/api/v1/mods/{id}")).await.0,
        StatusCode::NOT_FOUND
    );
    assert!(
        json(&public.get("/manifest.json").await.2)["mods"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(alice.get(&page).await.0, StatusCode::OK);
    assert_eq!(
        alice
            .post(&format!("/mods/{id}/visibility"), &[("published", "true")])
            .await
            .0,
        StatusCode::SEE_OTHER
    );
    alice.app = router_with_releases(config.clone(), releases::ReleaseCache::offline()).unwrap();
    assert_eq!(alice.get("/dashboard").await.0, StatusCode::OK);
    let catalogue = json(&alice.get("/api/v1/mods").await.2);
    assert_eq!(catalogue["mods"][0]["releases"][0]["version"], "1.1.0");
    assert_eq!(
        catalogue["mods"][0]["releases"].as_array().unwrap().len(),
        2
    );
    assert_eq!(public.get(url).await.2, zip);
    let conn = Connection::open(config.data.join("website.sqlite3")).unwrap();
    let encoded: String = conn
        .query_row(
            "SELECT password_hash FROM users WHERE username = 'alice'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(encoded.starts_with("$argon2id$"));
    let stale_session = alice.cookies.get("resonance_session").unwrap().clone();
    assert_eq!(
        alice.post("/account/logout", &[]).await.0,
        StatusCode::SEE_OTHER
    );
    alice
        .cookies
        .insert("resonance_session".into(), stale_session);
    assert_eq!(alice.get("/dashboard").await.0, StatusCode::SEE_OTHER);
    assert_eq!(
        alice
            .post(
                "/account/login",
                &[("username", "alice"), ("password", "incorrect password")]
            )
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        alice
            .post(
                "/account/login",
                &[("username", "nobody"), ("password", "incorrect password")]
            )
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        alice
            .post(
                "/account/login",
                &[
                    ("username", "ALICE"),
                    ("password", "correct horse battery staple")
                ]
            )
            .await
            .0,
        StatusCode::SEE_OTHER
    );
    let token = alice.cookies.get("resonance_session").unwrap();
    conn.execute(
        "UPDATE sessions SET expires_at = 0 WHERE token_hash = ?1",
        [hash(token)],
    )
    .unwrap();
    assert_eq!(alice.get("/dashboard").await.0, StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn forms_require_csrf_and_responses_enforce_browser_security() {
    let (_data, _, app) = setup();
    let mut browser = Browser::new(app);
    let (_, headers, body) = browser.get("/account/register").await;
    assert!(text(&body).contains("Create an account"));
    assert!(
        headers[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .contains("HttpOnly; SameSite=Lax")
    );
    assert!(
        headers[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .contains("Secure")
    );
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert!(
        headers["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("frame-ancestors 'none'")
    );
    assert_eq!(
        browser
            .request(
                "POST",
                "/account/register",
                "application/x-www-form-urlencoded",
                b"username=alice&password=correct+horse+battery+staple&csrf=fake".to_vec()
            )
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        browser.get("/account/unknown").await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(browser.get("/not-a-page").await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        browser
            .get("/api/v1/releases/../../schema.sql/download")
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let request = Request::builder()
        .method("POST")
        .uri("/mods/new")
        .header(header::CONTENT_LENGTH, PART_BYTES + 1)
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        browser.app.oneshot(request).await.unwrap().status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
}

#[tokio::test]
async fn stalled_mutations_are_bounded_without_blocking_public_routes() {
    let (_data, _config, app) = setup();
    let mut senders = Vec::new();
    let mut requests = Vec::new();
    for _ in 0..8 {
        let (sender, body) =
            http_body_util::channel::Channel::<axum::body::Bytes, std::io::Error>::new(1);
        let request = Request::builder()
            .method("POST")
            .uri("/account/login")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::new(body))
            .unwrap();
        let mut response = Box::pin(app.clone().oneshot(request));
        std::future::poll_fn(|cx| {
            assert!(response.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        senders.push(sender);
        requests.push(response);
    }
    let mut browser = Browser::new(app);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        for path in ["/healthz", "/", "/manifest.json", "/static/style.css"] {
            assert_eq!(browser.get(path).await.0, StatusCode::OK, "{path}");
        }
        assert_eq!(
            browser.get("/download/windows-x64").await.0,
            StatusCode::SEE_OTHER
        );
        for path in ["/account/login", "/api/v1/uploads"] {
            let response = browser.post(path, &[]).await;
            assert_eq!(response.0, StatusCode::TOO_MANY_REQUESTS);
            assert_eq!(response.1[header::CACHE_CONTROL], "no-store");
            if path.starts_with("/api/") {
                assert!(json(&response.2)["error"].is_string());
            }
        }
    })
    .await
    .expect("stalled mutations blocked unrelated requests");

    // Finishing one stalled body makes room for another mutation.
    drop(senders.remove(0));
    assert_eq!(
        requests.remove(0).await.unwrap().status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        browser.post("/account/login", &[]).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[tokio::test(start_paused = true)]
async fn stalled_form_and_json_bodies_time_out_before_entering_handlers() {
    let (_data, _config, app) = setup();
    for (path, content_type) in [
        ("/account/login", "application/x-www-form-urlencoded"),
        ("/api/v1/uploads", "application/json"),
    ] {
        let (_sender, body) =
            http_body_util::channel::Channel::<axum::body::Bytes, std::io::Error>::new(1);
        let request = Request::builder()
            .method("POST")
            .uri(path)
            .header(header::CONTENT_TYPE, content_type)
            .body(Body::new(body))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT, "{path}");
    }
    let mut browser = Browser::new(app);
    browser.get("/account/login").await;
    assert_eq!(
        browser
            .post(
                "/account/login",
                &[
                    ("username", "unknown"),
                    ("password", "long enough password"),
                ]
            )
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}

#[test]
fn cancelled_mutations_retain_capacity_until_their_work_finishes() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    runtime.block_on(async {
        let (_data, config, app) = setup();
        let mut browser = Browser::new(app);
        browser.get("/account/login").await;
        let (release_worker, wait) = std::sync::mpsc::channel();
        let (started, ready) = tokio::sync::oneshot::channel();
        let worker = tokio::task::spawn_blocking(move || {
            started.send(()).unwrap();
            wait.recv().unwrap();
        });
        ready.await.unwrap();
        let credentials = [
            ("username", "unknown"),
            ("password", "long enough password"),
        ];
        for _ in 0..8 {
            let mut request = Box::pin(browser.post("/account/login", &credentials));
            std::future::poll_fn(|cx| {
                assert!(request.as_mut().poll(cx).is_pending());
                std::task::Poll::Ready(())
            })
            .await;
            drop(request);
            tokio::task::yield_now().await;
        }
        // Public requests still work; cancelled mutations must still occupy all slots.
        let health = browser.get("/healthz").await.0;
        let ninth = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            browser.post("/account/login", &credentials),
        )
        .await;
        // Release the worker before asserting, so failures cannot hang runtime shutdown.
        release_worker.send(()).unwrap();
        worker.await.unwrap();
        assert_eq!(health, StatusCode::OK);
        assert_eq!(
            ninth.expect("ninth mutation was queued").0,
            StatusCode::TOO_MANY_REQUESTS
        );

        // The accepted operations finish despite cancellation, then release capacity.
        let response = browser.get("/account/login").await;
        assert_eq!(response.0, StatusCode::OK);
        let attempts: i64 = open_db(&config)
            .unwrap()
            .query_row("SELECT MAX(attempts) FROM auth_attempts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(attempts, 8);
        assert_eq!(
            browser.post("/account/login", &credentials).await.0,
            StatusCode::UNAUTHORIZED
        );
    });
}

#[tokio::test]
async fn account_requests_have_small_body_limits_and_persistent_throttling() {
    let (_data, mut config, app) = setup();
    let mut browser = Browser::new(app);
    browser.get("/account/login").await;
    assert_eq!(
        browser
            .request(
                "POST",
                "/account/login",
                "application/x-www-form-urlencoded",
                vec![b'a'; 16 * 1024 + 1]
            )
            .await
            .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    for _ in 0..15 {
        assert_eq!(
            browser
                .post(
                    "/account/login",
                    &[("username", "!"), ("password", "irrelevant password")]
                )
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    browser.app = router_with_releases(config.clone(), releases::ReleaseCache::offline()).unwrap();
    browser.cookies.clear();
    browser.get("/account/login").await;
    assert_eq!(
        browser
            .post(
                "/account/login",
                &[("username", "!"), ("password", "irrelevant password")]
            )
            .await
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    // Changing usernames or spoofing a forwarded address cannot reset the
    // independent client allowance. The first 16 attempts were already charged.
    for i in 0..44 {
        browser.client_ip = Some(format!("198.51.100.{}", i + 1));
        assert_eq!(
            browser
                .post(
                    "/account/login",
                    &[("username", &format!("user{i}")), ("password", "short")]
                )
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    browser.app = router_with_releases(config.clone(), releases::ReleaseCache::offline()).unwrap();
    for path in ["/account/login", "/account/%6cogin", "/account/%72egister"] {
        assert_eq!(
            browser
                .post(path, &[("username", "another"), ("password", "short")])
                .await
                .0,
            StatusCode::TOO_MANY_REQUESTS,
            "{path}"
        );
    }

    // Caddy explicitly overwrites the single client-IP header; clients behind
    // that proxy must have independent counters for the same account.
    config.trusted_proxies = vec!["127.0.0.1".parse().unwrap()];
    let app = router_with_releases(config, releases::ReleaseCache::offline()).unwrap();
    let mut attacker = Browser::new(app.clone());
    let mut victim = Browser::new(app);
    victim.client_ip = Some("198.51.100.2".into());
    victim.register("victim").await;
    attacker.get("/account/login").await;
    for value in [
        None,
        Some("invalid".into()),
        Some("198.51.100.1, 198.51.100.2".into()),
    ] {
        attacker.client_ip = value;
        for path in ["/account/login", "/account/%6cogin", "/account/%72egister"] {
            let response = attacker
                .post(
                    path,
                    &[("username", "victim"), ("password", "wrong password")],
                )
                .await;
            assert_eq!(response.0, StatusCode::BAD_REQUEST);
            assert!(text(&response.2).contains("X-Resonance-Client-IP"));
            assert_eq!(response.1[header::CACHE_CONTROL], "no-store");
        }
    }
    attacker.client_ip = Some("198.51.100.1".into());
    for _ in 0..15 {
        assert_eq!(
            attacker
                .post(
                    "/account/login",
                    &[("username", "victim"), ("password", "short")]
                )
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    for path in ["/account/login", "/account/%6cogin", "/account/%72egister"] {
        assert_eq!(
            attacker
                .post(path, &[("username", "victim"), ("password", "short")])
                .await
                .0,
            StatusCode::TOO_MANY_REQUESTS,
            "{path}"
        );
    }
    assert_eq!(
        victim
            .post(
                "/account/login",
                &[
                    ("username", "victim"),
                    ("password", "correct horse battery staple")
                ]
            )
            .await
            .0,
        StatusCode::SEE_OTHER
    );
}

fn account_id(config: &Config, username: &str) -> i64 {
    Connection::open(config.data.join("website.sqlite3"))
        .unwrap()
        .query_row(
            "SELECT id FROM users WHERE username = ?1",
            [username],
            |r| r.get(0),
        )
        .unwrap()
}
fn account_limits(config: &Config, username: &str) -> UploadLimits {
    UploadLimits::load(
        &Connection::open(config.data.join("website.sqlite3")).unwrap(),
        account_id(config, username),
    )
    .unwrap()
}
#[tokio::test]
async fn admins_set_upload_limits_directly_and_settings_persist() {
    let (_data, config, app) = setup();
    let mut author = Browser::new(app.clone());
    let mut admin = Browser::new(app);
    author.register("author").await;
    approve_uploader(&config, "author");
    admin.register("maintainer").await;
    set_admin(&config, "maintainer", true).unwrap();
    let changes = [
        ("zip_mib", "512"),
        ("expanded_mib", "2048"),
        ("quota_mib", "4096"),
    ];
    assert_eq!(
        author.policy(&config, "author", &changes).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        admin
            .policy(
                &config,
                "author",
                &[("zip_mib", "999999"), ("quota_mib", "4096")]
            )
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        moderation::policy(&open_db(&config).unwrap(), account_id(&config, "author"))
            .unwrap()
            .quota_mib,
        512
    );
    assert_eq!(
        admin.policy(&config, "author", &changes).await.0,
        StatusCode::SEE_OTHER
    );
    author.app = router_with_releases(config.clone(), releases::ReleaseCache::offline()).unwrap();
    assert!(text(&author.get("/mods/new").await.2).contains("Maximum 512 MiB"));
    assert_eq!(
        account_limits(&config, "author"),
        UploadLimits {
            zip_mib: 512,
            expanded_mib: 2048
        }
    );
    assert_eq!(
        admin
            .policy(
                &config,
                "author",
                &[("zip_mib", "64"), ("expanded_mib", "256")]
            )
            .await
            .0,
        StatusCode::SEE_OTHER
    );
    assert_eq!(account_limits(&config, "author"), UploadLimits::default());
}

async fn upload_file(
    browser: &mut Browser,
    archive: &std::path::Path,
    mod_id: &str,
    version: &str,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut file = std::fs::File::open(archive).unwrap();
    let size = file.metadata().unwrap().len();
    let mut first = vec![0u8; PART_BYTES.min(size) as usize];
    file.read_exact(&mut first).unwrap();
    let last_at = ((size - 1) / PART_BYTES) * PART_BYTES;
    file.seek(std::io::SeekFrom::Start(last_at)).unwrap();
    let mut last = Vec::new();
    file.read_to_end(&mut last).unwrap();
    let response = browser
        .start_upload(
            mod_id,
            version,
            size,
            format!("{}{}", hash(&first), hash(&last)),
        )
        .await;
    if response.0 != StatusCode::OK {
        return response;
    }
    let id = json(&response.2)["id"].as_str().unwrap().to_owned();
    file.rewind().unwrap();
    for part in 1..=size.div_ceil(PART_BYTES) as u32 {
        let mut bytes = vec![0; crate::model::part_length(size, part).unwrap()];
        file.read_exact(&mut bytes).unwrap();
        let response = browser
            .request(
                "PUT",
                &format!("/api/v1/uploads/{id}/parts/{part}"),
                "application/octet-stream",
                bytes,
            )
            .await;
        if response.0 != StatusCode::OK {
            return response;
        }
    }
    browser.finish(&id).await
}

#[tokio::test]
async fn archives_over_64_mib_stream_only_after_approval_and_cleanup_on_failure() {
    let (_data, config, app) = setup();
    let mut author = Browser::new(app.clone());
    let mut admin = Browser::new(app.clone());
    author.register("textures").await;
    approve_uploader(&config, "textures");
    admin.register("maintainer").await;
    set_admin(&config, "maintainer", true).unwrap();
    let mut archive = tempfile::NamedTempFile::new().unwrap();
    {
        let mut writer = zip::ZipWriter::new(archive.as_file_mut());
        writer
            .start_file(
                "ui/hd-textures.ktx2",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
        let chunk = [0x5au8; 64 * 1024];
        for _ in 0..1028 {
            writer.write_all(&chunk).unwrap();
        }
        writer.finish().unwrap();
    }
    archive.flush().unwrap();
    assert!(archive.as_file().metadata().unwrap().len() > 64 * MIB);
    assert_eq!(
        upload_file(&mut author, archive.path(), "", "1.0.0")
            .await
            .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        std::fs::read_dir(config.data.join("uploads"))
            .unwrap()
            .count(),
        0
    );
    assert_eq!(
        admin
            .policy(
                &config,
                "textures",
                &[("zip_mib", "128"), ("expanded_mib", "512"),]
            )
            .await
            .0,
        StatusCode::SEE_OTHER
    );
    let (status, _, body) = upload_file(&mut author, archive.path(), "", "1.0.0").await;
    assert_eq!(status, StatusCode::OK, "{}", text(&body));
    let uploaded = json(&body);
    let mod_id = uploaded["mod_id"].as_str().unwrap();
    approve_releases(&config);
    let catalogue = json(&author.get("/api/v1/mods").await.2);
    let release = &catalogue["mods"][0]["releases"][0];
    assert_eq!(release["size"], archive.as_file().metadata().unwrap().len());
    let path = release["download_url"]
        .as_str()
        .unwrap()
        .strip_prefix(&config.public_url)
        .unwrap();
    let response = app
        .clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body();
    let mut digest = Sha256::new();
    while let Some(frame) = body.frame().await {
        if let Ok(data) = frame.unwrap().into_data() {
            digest.update(data);
        }
    }
    assert_eq!(release["sha256"], hex::encode(digest.finalize()));
    assert_eq!(
        admin
            .policy(
                &config,
                "textures",
                &[("zip_mib", "64"), ("expanded_mib", "256"),]
            )
            .await
            .0,
        StatusCode::SEE_OTHER
    );
    assert_eq!(
        upload_file(&mut author, archive.path(), mod_id, "1.1.0")
            .await
            .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let old_download = app
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(old_download.status(), StatusCode::OK);
}

#[test]
fn approved_limits_cover_expanded_size_and_file_counts() {
    let mut archive = tempfile::NamedTempFile::new().unwrap();
    {
        let mut writer = zip::ZipWriter::new(archive.as_file_mut());
        writer
            .start_file(
                "ui/large-texture.ktx2",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated)
                    .compression_level(Some(1)),
            )
            .unwrap();
        let chunk = [0u8; 64 * 1024];
        for _ in 0..4097 {
            writer.write_all(&chunk).unwrap();
        }
        writer.finish().unwrap();
    }
    assert_eq!(
        validate_archive(archive.reopen().unwrap(), UploadLimits::default())
            .err()
            .unwrap()
            .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert!(
        validate_archive(
            archive.reopen().unwrap(),
            UploadLimits {
                zip_mib: 128,
                expanded_mib: 512
            }
        )
        .is_ok()
    );
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..4097 {
        writer
            .start_file(
                format!("ui/textures/{i}.ktx2"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(b"texture").unwrap();
    }
    let many_files = writer.finish().unwrap().into_inner();
    assert!(validate_archive(Cursor::new(&many_files), UploadLimits::default()).is_err());
    assert!(
        validate_archive(
            Cursor::new(&many_files),
            UploadLimits {
                zip_mib: 128,
                expanded_mib: 512
            }
        )
        .is_ok()
    );
}

#[tokio::test]
async fn uploads_resume_after_restart_and_enforce_part_ownership_and_order() {
    let (_data, config, app) = setup();
    let mut owner = Browser::new(app.clone());
    let mut other = Browser::new(app);
    owner.register("uploader").await;
    approve_uploader(&config, "uploader");
    other.register("other").await;
    approve_uploader(&config, "other");
    let zip = archive(&[("ui/texture.ktx2", &vec![0x55; PART_BYTES as usize + 50])]);
    let parts: Vec<_> = zip.chunks(PART_BYTES as usize).collect();
    let fingerprint = format!("{}{}", hash(parts[0]), hash(parts[1]));
    let response = owner
        .start_upload("", "1.0.0", zip.len() as u64, fingerprint.clone())
        .await;
    assert_eq!(response.0, StatusCode::OK, "{}", text(&response.2));
    let upload = json(&response.2);
    let id = upload["id"].as_str().unwrap();
    let path = format!("/api/v1/uploads/{id}");
    assert_eq!(
        owner
            .start_upload("", "1.0.0", zip.len() as u64, fingerprint)
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(other.get(&path).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        other.post(&format!("{path}/abort"), &[]).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        other
            .request("PUT", &format!("{path}/parts/1"), "", parts[0].to_vec())
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        owner.post(&format!("{path}/complete"), &[]).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        owner
            .request("PUT", &format!("{path}/parts/2"), "", parts[1].to_vec())
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        owner
            .request(
                "PUT",
                &format!("{path}/parts/1"),
                "",
                vec![0; PART_BYTES as usize]
            )
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let response = owner
        .request("PUT", &format!("{path}/parts/1"), "", parts[0].to_vec())
        .await;
    assert_eq!(response.0, StatusCode::OK, "{}", text(&response.2));
    let retry = owner
        .request("PUT", &format!("{path}/parts/1"), "", parts[0].to_vec())
        .await;
    assert_eq!(retry.0, StatusCode::OK);
    assert_eq!(json(&retry.2)["received"], PART_BYTES);
    let archive_path = config.data.join("uploads").join(format!("{id}.zip"));
    // A crash can leave bytes on disk before their receipt reaches SQLite.
    std::fs::OpenOptions::new()
        .append(true)
        .open(&archive_path)
        .unwrap()
        .write_all(b"uncommitted write")
        .unwrap();
    owner.app = router_with_releases(config.clone(), releases::ReleaseCache::offline()).unwrap();
    let resumed = json(&owner.get("/api/v1/uploads/active").await.2);
    assert_eq!(resumed["upload"]["id"], id);
    assert_eq!(resumed["upload"]["next_part"], 2);
    assert_eq!(
        owner
            .request("PUT", &format!("{path}/parts/2"), "", parts[1].to_vec())
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(std::fs::read(&archive_path).unwrap(), zip);
    // Recover a validation interrupted before it could publish.
    let conn = Connection::open(config.data.join("website.sqlite3")).unwrap();
    conn.execute("UPDATE uploads SET status='validating' WHERE id=?1", [id])
        .unwrap();
    owner.app = router_with_releases(config.clone(), releases::ReleaseCache::offline()).unwrap();
    assert_eq!(owner.finish(id).await.0, StatusCode::OK);
    approve_releases(&config);
    assert_eq!(
        owner.post(&format!("{path}/complete"), &[]).await.0,
        StatusCode::OK
    );
    assert_eq!(
        owner.post(&format!("{path}/abort"), &[]).await.0,
        StatusCode::CONFLICT
    );
    let catalogue = json(&owner.get("/api/v1/mods").await.2);
    let release = &catalogue["mods"][0]["releases"][0];
    assert_eq!(release["sha256"], hash(&zip));
    assert_eq!(release["size"], zip.len());
    let url = release["download_url"]
        .as_str()
        .unwrap()
        .strip_prefix(&config.public_url)
        .unwrap();
    let response = owner
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(url)
                .header(header::RANGE, "bytes=10-99")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        response.headers()[header::CONTENT_RANGE],
        format!("bytes 10-99/{}", zip.len())
    );
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        &zip[10..100]
    );
    let response = owner
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("HEAD")
                .uri(url)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_LENGTH],
        zip.len().to_string()
    );
    assert!(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .is_empty()
    );
    conn.execute("UPDATE uploads SET expires_at=0", []).unwrap();
    owner.app = router_with_releases(config, releases::ReleaseCache::offline()).unwrap();
    assert!(archive_path.exists());
    assert_eq!(owner.get(url).await.2, zip);
}

#[tokio::test]
async fn uploads_reclaim_uncommitted_bytes_before_checking_disk_space() {
    let (_data, mut config, app) = setup();
    let mut owner = Browser::new(app);
    owner.register("owner").await;
    approve_uploader(&config, "owner");
    let zip = archive(&[("ui/layout.json", b"{}")]);
    let response = owner
        .start_upload("", "1.0.0", zip.len() as u64, hash(&zip).repeat(2))
        .await;
    assert_eq!(response.0, StatusCode::OK);
    let upload = json(&response.2);
    let id = upload["id"].as_str().unwrap();
    let path = config.data.join("uploads").join(format!("{id}.zip"));
    let part_url = format!("/api/v1/uploads/{id}/parts/1");
    // Model a crash after writing a part but before committing its receipt.
    std::fs::write(&path, &zip).unwrap();
    // Force disk admission to fail without relying on changing host free space.
    config.disk_reserve_bytes = u64::MAX;
    owner.app = router_with_releases(config.clone(), releases::ReleaseCache::offline()).unwrap();
    assert_eq!(
        owner.request("PUT", &part_url, "", zip.clone()).await.0,
        StatusCode::INSUFFICIENT_STORAGE
    );
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
    let status = json(&owner.get(&format!("/api/v1/uploads/{id}")).await.2);
    assert_eq!(status["received"], 0);
    assert_eq!(status["next_part"], 1);

    config.disk_reserve_bytes = 0;
    owner.app = router_with_releases(config, releases::ReleaseCache::offline()).unwrap();
    assert_eq!(
        owner.request("PUT", &part_url, "", zip.clone()).await.0,
        StatusCode::OK
    );
    assert_eq!(std::fs::read(&path).unwrap(), zip);
    assert_eq!(owner.finish(id).await.0, StatusCode::OK);
}

#[tokio::test]
async fn failed_cancelled_and_expired_uploads_leave_no_public_files() {
    let (_data, config, app) = setup();
    let mut owner = Browser::new(app);
    owner.register("uploader").await;
    approve_uploader(&config, "uploader");
    let invalid = b"not a zip";
    let failed = owner.upload("", "1.0.0", invalid).await;
    assert_eq!(failed.0, StatusCode::BAD_REQUEST);
    assert_eq!(json(&failed.2)["status"], "failed");
    assert_eq!(
        std::fs::read_dir(config.data.join("uploads"))
            .unwrap()
            .count(),
        0
    );
    let zip = archive(&[("ui/good.ktx2", b"texture")]);
    let fingerprint = format!("{}{}", hash(&zip), hash(&zip));
    let upload = json(
        &owner
            .start_upload("", "1.0.0", zip.len() as u64, fingerprint.clone())
            .await
            .2,
    );
    let id = upload["id"].as_str().unwrap();
    let path = format!("/api/v1/uploads/{id}");
    // A session cookie alone cannot authorize upload mutations.
    let response = owner
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("{path}/abort"))
                .header(
                    header::COOKIE,
                    format!("resonance_session={}", owner.cookies["resonance_session"]),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(
        response.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("application/json")
    );
    assert_eq!(
        owner
            .request(
                "PUT",
                &format!("{path}/parts/1"),
                "",
                vec![0; PART_BYTES as usize + 1]
            )
            .await
            .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        owner
            .request("PUT", &format!("{path}/parts/1"), "", zip.clone())
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        owner.post(&format!("{path}/abort"), &[]).await.0,
        StatusCode::OK
    );
    assert_eq!(
        owner.post(&format!("{path}/complete"), &[]).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        std::fs::read_dir(config.data.join("uploads"))
            .unwrap()
            .count(),
        0
    );
    let upload = json(
        &owner
            .start_upload("", "1.0.0", zip.len() as u64, fingerprint)
            .await
            .2,
    );
    let id = upload["id"].as_str().unwrap();
    let conn = Connection::open(config.data.join("website.sqlite3")).unwrap();
    conn.execute("UPDATE uploads SET expires_at=0 WHERE id=?1", [id])
        .unwrap();
    assert_eq!(
        owner
            .request("PUT", &format!("/api/v1/uploads/{id}/parts/1"), "", zip)
            .await
            .0,
        StatusCode::GONE
    );
    owner.app = router_with_releases(config.clone(), releases::ReleaseCache::offline()).unwrap();
    assert_eq!(
        std::fs::read_dir(config.data.join("uploads"))
            .unwrap()
            .count(),
        0
    );
    assert_eq!(
        json(&owner.get("/api/v1/uploads/active").await.2)["upload"],
        Value::Null
    );
    assert!(
        json(&owner.get("/manifest.json").await.2)["mods"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    for path in [
        "/data/website.sqlite3",
        "/uploads/example.zip",
        "/secrets/origin.key",
    ] {
        assert_eq!(owner.get(path).await.0, StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
async fn closed_registration_blocks_new_accounts_and_preserves_existing_login() {
    let (_data, mut config, app) = setup();
    let mut browser = Browser::new(app);
    browser.register("existing").await;
    config.registration_open = false;
    browser.app = router_with_releases(config.clone(), releases::ReleaseCache::offline()).unwrap();
    // Closing registration must not revoke current sessions.
    assert_eq!(browser.get("/dashboard").await.0, StatusCode::OK);
    assert_eq!(
        browser.post("/account/logout", &[]).await.0,
        StatusCode::SEE_OTHER
    );
    for path in ["/", "/account/login"] {
        let response = browser.get(path).await;
        assert_eq!(response.0, StatusCode::OK);
        assert!(!text(&response.2).contains("/account/register"));
    }
    let response = browser.get("/account/register").await;
    assert_eq!(response.0, StatusCode::FORBIDDEN);
    assert!(text(&response.2).contains("Account creation is currently closed."));
    // Send a valid form and CSRF token directly, without using a sign-up link.
    let response = browser
        .post(
            "/account/register",
            &[
                ("username", "new-account"),
                ("password", "correct horse battery staple"),
            ],
        )
        .await;
    assert_eq!(response.0, StatusCode::FORBIDDEN);
    assert!(text(&response.2).contains("Account creation is currently closed."));
    let conn = Connection::open(config.data.join("website.sqlite3")).unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM users", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        browser
            .post(
                "/account/login",
                &[
                    ("username", "existing"),
                    ("password", "correct horse battery staple")
                ]
            )
            .await
            .0,
        StatusCode::SEE_OTHER
    );
    assert_eq!(browser.get("/dashboard").await.0, StatusCode::OK);
    // The switch can be reopened later without removing the registration flow.
    config.registration_open = true;
    browser.app = router_with_releases(config, releases::ReleaseCache::offline()).unwrap();
    browser.register("new-account").await;
}

fn approve_uploader(config: &Config, name: &str) {
    let conn = Connection::open(config.data.join("website.sqlite3")).unwrap();
    conn.execute("INSERT OR REPLACE INTO author_policy (user_id,status) SELECT id,'approved' FROM users WHERE username=?1",[name]).unwrap();
}
fn approve_releases(config: &Config) {
    Connection::open(config.data.join("website.sqlite3"))
        .unwrap()
        .execute("UPDATE release_reviews SET status='approved'", [])
        .unwrap();
}
async fn set_access(
    admin: &mut Browser,
    config: &Config,
    name: &str,
    status: &str,
    quota: &str,
    daily: &str,
) {
    let response = admin
        .policy(
            config,
            name,
            &[
                ("status", status),
                ("quota_mib", quota),
                ("daily_mib", daily),
            ],
        )
        .await;
    assert_eq!(response.0, StatusCode::SEE_OTHER, "{}", text(&response.2));
}
async fn review_release(admin: &mut Browser, id: &str, decision: &str) {
    let response = admin
        .post(
            &format!("/admin/releases/{id}/review"),
            &[("decision", decision), ("note", "Reviewed <details>")],
        )
        .await;
    assert_eq!(response.0, StatusCode::SEE_OTHER, "{}", text(&response.2));
}
#[tokio::test]
async fn moderation_controls_accounts_reviews_downloads_suspension_and_deletion() {
    let (_data, config, app) = setup();
    let mut admin = Browser::new(app.clone());
    let mut author = Browser::new(app.clone());
    let mut other = Browser::new(app.clone());
    let mut public = Browser::new(app);
    admin.register("admin").await;
    author.register("author").await;
    other.register("other").await;
    set_admin(&config, "admin", true).unwrap();
    let zip = archive(&[("ui/layout.json", br#"{"colour":"blue"}"#)]);
    assert_eq!(
        author.upload("", "1.0.0", &zip).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(author.get("/mods/new").await.0, StatusCode::FORBIDDEN);
    assert_eq!(
        author
            .post(
                "/account/upload-access",
                &[("reason", "Original interface textures <script>")]
            )
            .await
            .0,
        StatusCode::SEE_OTHER
    );
    assert_eq!(
        author
            .post("/account/upload-access", &[("reason", "Another request")])
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(author.get("/admin").await.0, StatusCode::FORBIDDEN);
    let author_path = format!("/admin/accounts/{}", account_id(&config, "author"));
    let fields = [
        ("status", "approved"),
        ("quota_mib", "512"),
        ("daily_mib", "2048"),
        ("zip_mib", "64"),
        ("expanded_mib", "256"),
    ];
    assert_eq!(
        author.post(&author_path, &fields).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        admin
            .request(
                "POST",
                &author_path,
                "application/x-www-form-urlencoded",
                b"csrf=bad&status=approved&quota_mib=512&daily_mib=2048&zip_mib=64&expanded_mib=256".to_vec()
            )
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    set_access(&mut admin, &config, "author", "approved", "512", "2048").await;
    let uploaded = author.upload("", "1.0.0", &zip).await;
    assert_eq!(uploaded.0, StatusCode::OK, "{}", text(&uploaded.2));
    let uploaded = json(&uploaded.2);
    let id = uploaded["id"].as_str().unwrap();
    let mod_id = uploaded["mod_id"].as_str().unwrap();
    let download = format!("/api/v1/releases/{id}/download");
    let page = format!("/mods/{mod_id}");
    for browser in [&mut public, &mut author] {
        assert_eq!(browser.get(&download).await.0, StatusCode::NOT_FOUND);
    }
    assert_eq!(
        public.request("HEAD", &download, "", vec![]).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(public.get(&page).await.0, StatusCode::NOT_FOUND);
    assert_eq!(author.get(&page).await.0, StatusCode::OK);
    assert!(
        json(&public.get("/manifest.json").await.2)["mods"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        author
            .post(
                &format!("/mods/{mod_id}/visibility"),
                &[("published", "true")]
            )
            .await
            .0,
        StatusCode::SEE_OTHER
    );
    assert_eq!(public.get(&download).await.0, StatusCode::NOT_FOUND);
    let private = format!("/admin/releases/{id}/download");
    assert_eq!(other.get(&private).await.0, StatusCode::FORBIDDEN);
    assert_eq!(admin.get(&private).await.2, zip);
    let review = format!("/admin/releases/{id}/review");
    assert_eq!(
        author
            .post(&review, &[("decision", "approve"), ("note", "")])
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let admin_page = admin.get("/admin").await;
    assert_eq!(admin_page.0, StatusCode::OK, "{}", text(&admin_page.2));
    assert!(text(&admin_page.2).contains("&lt;script&gt;"));
    review_release(&mut admin, id, "approve").await;
    assert_eq!(public.get(&download).await.2, zip);
    assert_eq!(
        admin
            .post(&review, &[("decision", "approve"), ("note", "")])
            .await
            .0,
        StatusCode::CONFLICT
    );
    let next = author.upload(mod_id, "1.1.0", &zip).await;
    assert_eq!(next.0, StatusCode::OK);
    let next = json(&next.2);
    let next_id = next["id"].as_str().unwrap();
    assert_eq!(
        json(&public.get("/manifest.json").await.2)["mods"][0]["releases"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        public
            .get(&format!("/api/v1/releases/{next_id}/download"))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    // Suspending an author stops an in-flight upload and hides earlier approved releases.
    let active = author
        .start_upload(
            mod_id,
            "1.2.0",
            zip.len() as u64,
            format!("{}{}", hash(&zip), hash(&zip)),
        )
        .await;
    assert_eq!(active.0, StatusCode::OK);
    let active = json(&active.2);
    let active_id = active["id"].as_str().unwrap();
    set_access(&mut admin, &config, "author", "suspended", "512", "2048").await;
    assert_eq!(
        author
            .request(
                "PUT",
                &format!("/api/v1/uploads/{active_id}/parts/1"),
                "",
                zip.clone()
            )
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert!(
        !config
            .data
            .join("uploads")
            .join(format!("{active_id}.zip"))
            .exists()
    );
    assert_eq!(public.get(&download).await.0, StatusCode::NOT_FOUND);
    assert_eq!(public.get(&page).await.0, StatusCode::NOT_FOUND);
    assert_eq!(author.get("/dashboard").await.0, StatusCode::OK);
    set_access(&mut admin, &config, "author", "approved", "512", "2048").await;
    assert_eq!(public.get(&download).await.0, StatusCode::OK);
    review_release(&mut admin, next_id, "reject").await;
    assert!(
        !config
            .data
            .join("uploads")
            .join(format!("{next_id}.zip"))
            .exists()
    );
    assert_eq!(
        admin
            .get(&format!("/admin/releases/{next_id}/download"))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert!(text(&author.get(&page).await.2).contains("Reviewed &lt;details&gt;"));
    let deletion = format!("/admin/mods/{mod_id}/delete");
    assert_eq!(
        author.post(&deletion, &[("confirm", "delete")]).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        admin.post(&deletion, &[("confirm", "wrong")]).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        admin.post(&deletion, &[("confirm", "delete")]).await.0,
        StatusCode::SEE_OTHER
    );
    assert_eq!(public.get(&download).await.0, StatusCode::NOT_FOUND);
    assert_eq!(author.get(&page).await.0, StatusCode::NOT_FOUND);
    let conn = Connection::open(config.data.join("website.sqlite3")).unwrap();
    assert_eq!(
        moderation::policy(&conn, account_id(&config, "author"))
            .unwrap()
            .used,
        0
    );
    assert_eq!(
        std::fs::read_dir(config.data.join("uploads"))
            .unwrap()
            .count(),
        0
    );
    // Roles take effect for existing sessions; the final admin cannot be removed.
    let self_role = format!("/admin/accounts/{}/role", account_id(&config, "admin"));
    assert_eq!(
        admin.post(&self_role, &[("admin", "false")]).await.0,
        StatusCode::BAD_REQUEST
    );
    let other_role = format!("/admin/accounts/{}/role", account_id(&config, "other"));
    assert_eq!(
        author.post(&other_role, &[("admin", "true")]).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        admin.post(&other_role, &[("admin", "true")]).await.0,
        StatusCode::SEE_OTHER
    );
    assert_eq!(other.get("/admin").await.0, StatusCode::OK);
    assert_eq!(
        other.post(&self_role, &[("admin", "false")]).await.0,
        StatusCode::SEE_OTHER
    );
    assert_eq!(admin.get("/admin").await.0, StatusCode::FORBIDDEN);
    assert_eq!(
        other.post(&other_role, &[("admin", "false")]).await.0,
        StatusCode::BAD_REQUEST
    );
    assert!(set_admin(&config, "other", false).is_err());
}

#[tokio::test]
async fn release_approval_allows_reduced_quota_but_requires_upload_access() {
    let (_data, config, app) = setup();
    let mut owner = Browser::new(app.clone());
    let mut admin = Browser::new(app.clone());
    let mut public = Browser::new(app);
    owner.register("owner").await;
    admin.register("admin").await;
    set_admin(&config, "admin", true).unwrap();
    set_access(&mut admin, &config, "owner", "approved", "3", "2048").await;
    let zip = archive(&[("ui/texture.ktx2", &vec![0; 2 * MIB as usize])]);
    let response = owner.upload("", "1.0.0", &zip).await;
    assert_eq!(response.0, StatusCode::OK);
    let upload = json(&response.2);
    let id = upload["id"].as_str().unwrap();
    let review_url = format!("/admin/releases/{id}/review");
    let download_url = format!("/api/v1/releases/{id}/download");

    set_access(&mut admin, &config, "owner", "suspended", "1", "2048").await;
    assert_eq!(
        admin
            .post(&review_url, &[("decision", "approve"), ("note", "")])
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(public.get(&download_url).await.0, StatusCode::NOT_FOUND);
    set_access(&mut admin, &config, "owner", "approved", "1", "2048").await;
    review_release(&mut admin, id, "approve").await;
    assert_eq!(public.get(&download_url).await.2, zip);
    assert_eq!(
        owner.upload("", "1.0.0", &zip).await.0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
}

#[tokio::test]
async fn quotas_disk_reservations_rates_and_cleanup_bound_storage() {
    let (_data, mut config, app) = setup();
    let mut owner = Browser::new(app.clone());
    let mut admin = Browser::new(app);
    owner.register("owner").await;
    admin.register("admin").await;
    set_admin(&config, "admin", true).unwrap();
    set_access(&mut admin, &config, "owner", "approved", "3", "16").await;
    let reserved = owner
        .start_upload("", "1.0.0", 2 * MIB, "a".repeat(128))
        .await;
    assert_eq!(reserved.0, StatusCode::OK);
    let reserved = json(&reserved.2);
    let reserved_id = reserved["id"].as_str().unwrap();
    set_access(&mut admin, &config, "owner", "approved", "1", "16").await;
    let page = owner.get("/mods/new").await;
    assert_eq!(page.0, StatusCode::OK);
    assert!(text(&page.2).contains("cancel an unfinished upload"));
    assert_eq!(
        owner
            .post(&format!("/api/v1/uploads/{reserved_id}/complete"), &[])
            .await
            .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        owner
            .post(&format!("/api/v1/uploads/{reserved_id}/abort"), &[])
            .await
            .0,
        StatusCode::OK
    );
    let id = account_id(&config, "owner");
    let conn = Connection::open(config.data.join("website.sqlite3")).unwrap();
    assert_eq!(moderation::policy(&conn, id).unwrap().used, 0);
    let fingerprint = "a".repeat(128);
    assert_eq!(
        owner
            .start_upload("", "1.0.0", MIB + 1, fingerprint.clone())
            .await
            .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let started = owner
        .start_upload("", "1.0.0", MIB, fingerprint.clone())
        .await;
    assert_eq!(started.0, StatusCode::OK);
    let started = json(&started.2);
    let upload = started["id"].as_str().unwrap();
    assert_eq!(moderation::policy(&conn, id).unwrap().used, MIB);
    // Invalid bodies still consume bandwidth allowance, and restart does not reset it.
    for _ in 0..2 {
        assert_eq!(
            owner
                .request(
                    "PUT",
                    &format!("/api/v1/uploads/{upload}/parts/1"),
                    "",
                    vec![0]
                )
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    owner.app = router_with_releases(config.clone(), releases::ReleaseCache::offline()).unwrap();
    assert_eq!(
        owner
            .request(
                "PUT",
                &format!("/api/v1/uploads/{upload}/parts/1"),
                "",
                vec![0]
            )
            .await
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    conn.execute("UPDATE uploads SET expires_at=0", []).unwrap();
    uploads::cleanup(&config, &conn).unwrap();
    assert_eq!(moderation::policy(&conn, id).unwrap().used, 0);
    assert_eq!(
        std::fs::read_dir(config.data.join("uploads"))
            .unwrap()
            .count(),
        0
    );
    config.disk_reserve_bytes = u64::MAX;
    owner.app = router_with_releases(config.clone(), releases::ReleaseCache::offline()).unwrap();
    assert_eq!(
        owner
            .start_upload("", "1.0.0", 1, fingerprint.clone())
            .await
            .0,
        StatusCode::INSUFFICIENT_STORAGE
    );
    assert_eq!(
        std::fs::read_dir(config.data.join("uploads"))
            .unwrap()
            .count(),
        0
    );
    config.disk_reserve_bytes = 0;
    owner.app = router_with_releases(config.clone(), releases::ReleaseCache::offline()).unwrap();
    conn.execute("DELETE FROM upload_rates", []).unwrap();
    for _ in 0..20 {
        assert_eq!(
            owner
                .start_upload("", "1.0.0", MIB + 1, fingerprint.clone())
                .await
                .0,
            StatusCode::PAYLOAD_TOO_LARGE
        );
    }
    assert_eq!(
        owner
            .start_upload("", "1.0.0", 1, fingerprint.clone())
            .await
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    conn.execute("DELETE FROM upload_rates", []).unwrap();
    // Pending, hidden, approved and older versions all consume the same quota.
    let zip = archive(&[("scripts/large.sym", &vec![b' '; 700_000])]);
    let response = owner.upload("", "1.0.0", &zip).await;
    assert_eq!(response.0, StatusCode::OK, "{}", text(&response.2));
    let release = json(&response.2);
    let mod_id = release["mod_id"].as_str().unwrap();
    assert_eq!(
        moderation::policy(&conn, id).unwrap().used,
        zip.len() as u64
    );
    assert_eq!(
        owner.upload(mod_id, "1.1.0", &zip).await.0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    review_release(&mut admin, release["id"].as_str().unwrap(), "approve").await;
    owner
        .post(
            &format!("/mods/{mod_id}/visibility"),
            &[("published", "false")],
        )
        .await;
    assert_eq!(
        owner.upload(mod_id, "1.1.0", &zip).await.0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    review_release(&mut admin, release["id"].as_str().unwrap(), "reject").await;
    assert_eq!(moderation::policy(&conn, id).unwrap().used, 0);
}

#[tokio::test]
async fn upload_start_expires_only_author_files_after_access_and_rate_checks() {
    let (_data, config, app) = setup();
    let mut owner = Browser::new(app);
    owner.register("owner").await;
    let owner_id = account_id(&config, "owner");
    let conn = Connection::open(config.data.join("website.sqlite3")).unwrap();
    conn.execute(
        "INSERT INTO users (username,password_hash) VALUES ('other','unused')",
        [],
    )
    .unwrap();
    let other_id = conn.last_insert_rowid();
    for (id, user) in [("expired-owner", owner_id), ("expired-other", other_id)] {
        conn.execute(
            "INSERT INTO uploads (id,owner_id,mod_id,metadata,size,fingerprint,expires_at) VALUES (?1,?2,'unused','{}',?3,'unused',0)",
            params![id, user, (2 * MIB) as i64],
        ).unwrap();
        std::fs::write(
            config.data.join("uploads").join(format!("{id}.zip")),
            b"old",
        )
        .unwrap();
    }
    conn.execute(
        "INSERT INTO file_deletions VALUES ('queued-other.zip',?1,3)",
        [other_id],
    )
    .unwrap();
    std::fs::write(config.data.join("uploads/queued-other.zip"), b"old").unwrap();
    let orphan = std::fs::File::create(config.data.join("uploads/orphan.zip")).unwrap();
    orphan
        .set_times(std::fs::FileTimes::new().set_modified(UNIX_EPOCH))
        .unwrap();
    let expired = config.data.join("uploads/expired-owner.zip");

    assert_eq!(
        owner
            .start_upload("", "1.0.0", MIB, "a".repeat(128))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert!(expired.exists());
    approve_uploader(&config, "owner");
    conn.execute(
        "UPDATE author_policy SET quota_mib=1 WHERE user_id=?1",
        [owner_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO upload_rates VALUES (?1,'starts',20,?2)",
        params![owner_id, now() + 3600],
    )
    .unwrap();
    assert_eq!(
        owner
            .start_upload("", "1.0.0", MIB, "a".repeat(128))
            .await
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert!(expired.exists());
    conn.execute("DELETE FROM upload_rates WHERE user_id=?1", [owner_id])
        .unwrap();

    // Expiration frees the author's reservation even after their quota was lowered.
    assert_eq!(
        owner
            .start_upload("", "1.0.0", MIB, "a".repeat(128))
            .await
            .0,
        StatusCode::OK
    );
    assert!(!expired.exists());
    assert_eq!(moderation::policy(&conn, owner_id).unwrap().used, MIB);
    for file in ["expired-other.zip", "queued-other.zip", "orphan.zip"] {
        assert!(
            config.data.join("uploads").join(file).exists(),
            "removed {file}"
        );
    }
    assert_eq!(
        moderation::policy(&conn, other_id).unwrap().used,
        2 * MIB + 3
    );
}

#[tokio::test]
async fn pending_metadata_and_concurrent_uploads_cannot_bypass_review_or_quota() {
    let (_data, config, app) = setup();
    let mut owner = Browser::new(app.clone());
    let mut admin = Browser::new(app.clone());
    let mut public = Browser::new(app);
    owner.register("owner").await;
    admin.register("admin").await;
    set_admin(&config, "admin", true).unwrap();
    set_access(&mut admin, &config, "owner", "approved", "1", "2048").await;
    let mut second = Browser {
        app: owner.app.clone(),
        cookies: owner.cookies.clone(),
        peer: owner.peer,
        client_ip: owner.client_ip.clone(),
    };
    let fp = "a".repeat(128);
    let (a, b) = tokio::join!(
        owner.start_upload("", "1.0.0", 700_000, fp.clone()),
        second.start_upload("", "1.0.0", 700_000, fp)
    );
    assert!(
        (a.0 == StatusCode::OK && b.0 == StatusCode::PAYLOAD_TOO_LARGE)
            || (b.0 == StatusCode::OK && a.0 == StatusCode::PAYLOAD_TOO_LARGE)
    );
    let active = json(if a.0 == StatusCode::OK { &a.2 } else { &b.2 });
    owner
        .post(
            &format!("/api/v1/uploads/{}/abort", active["id"].as_str().unwrap()),
            &[],
        )
        .await;
    let zip = archive(&[("game/settings.json", b"{}")]);
    let first = owner.upload("", "1.0.0", &zip).await;
    assert_eq!(first.0, StatusCode::OK);
    let first = json(&first.2);
    let id = first["id"].as_str().unwrap();
    let mod_id = first["mod_id"].as_str().unwrap();
    review_release(&mut admin, id, "approve").await;
    let mut metadata = first["metadata"].clone();
    metadata["mod_id"] = json!(mod_id);
    metadata["title"] = json!("Unreviewed replacement");
    metadata["version"] = json!("1.1.0");
    let next=owner.request("POST","/api/v1/uploads","application/json",serde_json::to_vec(&json!({"metadata":metadata,"size":zip.len(),"fingerprint":format!("{}{}",hash(&zip),hash(&zip))})).unwrap()).await;
    assert_eq!(next.0, StatusCode::OK);
    let next = json(&next.2);
    let next_id = next["id"].as_str().unwrap();
    assert_eq!(
        owner
            .request(
                "PUT",
                &format!("/api/v1/uploads/{next_id}/parts/1"),
                "",
                zip
            )
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(owner.finish(next_id).await.0, StatusCode::OK);
    let before = json(&public.get("/manifest.json").await.2);
    assert_eq!(before["mods"][0]["title"], "Moonlit battle");
    assert_eq!(before["mods"][0]["releases"][0]["version"], "1.0.0");
    assert_eq!(before["mods"][0]["releases"][0]["review_note"], "");
    review_release(&mut admin, next_id, "approve").await;
    assert_eq!(
        json(&public.get("/manifest.json").await.2)["mods"][0]["title"],
        "Unreviewed replacement"
    );
    review_release(&mut admin, next_id, "reject").await;
    assert_eq!(
        json(&public.get("/manifest.json").await.2)["mods"][0]["title"],
        "Moonlit battle"
    );
    // Removing the review table models a database created before moderation existed.
    let conn = Connection::open(config.data.join("website.sqlite3")).unwrap();
    conn.execute("DELETE FROM release_reviews WHERE release_id=?1", [id])
        .unwrap();
    owner.app = router_with_releases(config.clone(), releases::ReleaseCache::offline()).unwrap();
    assert_eq!(
        owner
            .get(&format!("/api/v1/releases/{id}/download"))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert!(
        config
            .data
            .join("uploads")
            .join(format!("{id}.zip"))
            .exists()
    );
}

#[test]
fn cleanup_recovers_orphans_and_retries_failed_file_deletions() {
    let (_data, config, _app) = setup();
    let conn = Connection::open(config.data.join("website.sqlite3")).unwrap();
    conn.execute(
        "INSERT INTO users (username,password_hash) VALUES ('owner','unused')",
        [],
    )
    .unwrap();
    let id = account_id(&config, "owner");
    // A filesystem failure keeps the deletion queued and included in the quota.
    let stuck = config.data.join("uploads/stuck.zip");
    std::fs::create_dir(&stuck).unwrap();
    conn.execute(
        "INSERT INTO uploads (id,owner_id,mod_id,metadata,size,fingerprint,expires_at) VALUES ('stuck',?1,'mod','{}',1024,'unused',0)",
        [id],
    )
    .unwrap();
    // Expiry queues the failure without preventing startup or later cleanups.
    let _app = router_with_releases(config.clone(), releases::ReleaseCache::offline()).unwrap();
    uploads::cleanup(&config, &conn).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM uploads WHERE id='stuck'", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
    assert_eq!(moderation::policy(&conn, id).unwrap().used, 1024);
    std::fs::remove_dir(&stuck).unwrap();
    std::fs::write(&stuck, b"old upload").unwrap();
    moderation::collect_files(&config, &conn, None).unwrap();
    assert!(!stuck.exists());
    assert_eq!(moderation::policy(&conn, id).unwrap().used, 0);
    let orphan = config.data.join("uploads/orphan.zip");
    let f = std::fs::File::create(&orphan).unwrap();
    f.set_times(std::fs::FileTimes::new().set_modified(UNIX_EPOCH))
        .unwrap();
    let fresh = config.data.join("uploads/fresh.zip");
    std::fs::write(&fresh, b"new upload").unwrap();
    uploads::cleanup(&config, &conn).unwrap();
    assert!(!orphan.exists());
    assert!(fresh.exists());
    // Every outstanding reservation counts toward server-wide disk admission.
    conn.execute("INSERT INTO uploads (id,owner_id,mod_id,metadata,size,fingerprint,expires_at) VALUES ('reserved',?1,'mod','{}',9223372036854775807,'unused',?2)",params![id,now()+3600]).unwrap();
    assert_eq!(
        moderation::disk_space(&config, &conn, 1).unwrap_err().0,
        StatusCode::INSUFFICIENT_STORAGE
    );
}

#[tokio::test]
async fn admin_bootstrap_works_with_registration_closed_without_overwriting_accounts() {
    let (_data, mut config, _app) = setup();
    config.registration_open = false;
    assert!(create_admin(&config, "owner", "short").is_err());
    assert!(create_admin(&config, "bad/name", "correct horse battery staple").is_err());
    // The CLI and server count UTF-8 bytes, so a valid password can have fewer
    // than ten characters. The rendered login form must accept it too.
    let password = "é".repeat(5);
    create_admin(&config, "Owner", &password).unwrap();
    assert!(create_admin(&config, "owner", "another valid password").is_err());
    assert_eq!(
        list_accounts(&config).unwrap(),
        vec![("owner".into(), true)]
    );
    let app = router_with_releases(config, releases::ReleaseCache::offline()).unwrap();
    let mut owner = Browser::new(app);
    let login = text(&owner.get("/account/login").await.2);
    let password_input = login
        .split('<')
        .find(|tag| tag.starts_with("input type=\"password\""))
        .unwrap()
        .split('>')
        .next()
        .unwrap();
    assert!(!password_input.contains("minlength="));
    assert_eq!(
        owner
            .post(
                "/account/login",
                &[("username", "owner"), ("password", &password)]
            )
            .await
            .0,
        StatusCode::SEE_OTHER
    );
    assert_eq!(owner.get("/admin").await.0, StatusCode::OK);
    assert_eq!(
        owner.get("/account/register").await.0,
        StatusCode::FORBIDDEN
    );
}
