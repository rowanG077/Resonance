use super::*;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const PLATFORMS: [(&str, &str); 4] = [
    ("windows-x64", "zip"),
    ("macos-arm64", "tar.gz"),
    ("linux-x64", "tar.gz"),
    ("linux-arm64", "tar.gz"),
];
pub(super) struct ReleaseCache {
    client: reqwest::Client,
    endpoint: String,
    cached: tokio::sync::Mutex<(Instant, Option<Value>)>,
}
impl ReleaseCache {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(3))
                .user_agent("Resonance-Website")
                .build()?,
            endpoint: "https://api.github.com/repos/rowanG077/Resonance/releases/latest".into(),
            cached: tokio::sync::Mutex::new((Instant::now(), None)),
        })
    }
    pub async fn latest(&self) -> Option<Value> {
        let mut cache = self.cached.lock().await;
        if cache.0 > Instant::now() {
            return cache.1.clone();
        }
        let value = async {
            let response = self
                .client
                .get(&self.endpoint)
                .header("Accept", "application/vnd.github+json")
                .send()
                .await
                .ok()?
                .error_for_status()
                .ok()?;
            let source: Value = response.json().await.ok()?;
            parse_release(source)
        }
        .await;
        *cache = (
            Instant::now() + Duration::from_secs(if value.is_some() { 600 } else { 60 }),
            value.clone(),
        );
        value
    }
    #[cfg(test)]
    pub fn offline() -> Self {
        let result = Self::new().unwrap();
        *result.cached.try_lock().unwrap() = (Instant::now() + Duration::from_secs(600), None);
        result
    }
}
fn parse_release(source: Value) -> Option<Value> {
    if source["draft"] != false || source["prerelease"] != false {
        return None;
    }
    let tag = source["tag_name"].as_str()?;
    semver::Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()?;
    let assets = source["assets"].as_array()?;
    let mut downloads = serde_json::Map::new();
    for (platform, extension) in PLATFORMS {
        let name = format!("resonance-{tag}-{platform}.{extension}");
        let url = format!("{REPO}/releases/download/{tag}/{name}");
        if let Some(asset) = assets.iter().find(|a| {
            a["name"] == name && a["browser_download_url"] == url && a["state"] == "uploaded"
        }) {
            downloads.insert(
                platform.to_owned(),
                json!({"url":url,"size":asset["size"].as_u64()?}),
            );
        }
    }
    Some(json!({"version":tag,"downloads":downloads}))
}
pub(super) async fn download(
    State(app): State<Shared>,
    Path(platform): Path<String>,
) -> Result<Response> {
    if !PLATFORMS.iter().any(|(p, _)| *p == platform) {
        return Err(Error(
            StatusCode::NOT_FOUND,
            "That platform could not be found.".into(),
        ));
    }
    let release = app.releases.latest().await;
    let fallback = format!("{REPO}/releases/latest");
    let url = release
        .as_ref()
        .and_then(|r| r["downloads"][&platform]["url"].as_str())
        .unwrap_or(&fallback);
    Ok(Redirect::to(url).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tower::ServiceExt;

    #[tokio::test]
    async fn github_downloads_select_game_archives_and_cache_success_and_failure() {
        let assets: Vec<_> = PLATFORMS
            .iter()
            .flat_map(|(platform, ext)| {
                ["-symbols", ""].map(|kind| {
                    let name = format!("resonance-v9.8.7-{platform}{kind}.{ext}");
                    json!({"name":name,"state":"uploaded","size":123*1024*1024,
                "browser_download_url":format!("{REPO}/releases/download/v9.8.7/{name}")})
                })
            })
            .collect();
        let source = json!({"tag_name":"v9.8.7","draft":false,"prerelease":false,"assets":assets});
        let mut missing_platforms = source.clone();
        missing_platforms["assets"] = json!([]);
        let mut unexpected = source.clone();
        for asset in unexpected["assets"].as_array_mut().unwrap() {
            asset["browser_download_url"] = json!("https://example.com/untrusted.zip");
        }
        for (status, body, has_release, has_downloads) in [
            (StatusCode::OK, source.to_string(), true, true),
            (StatusCode::OK, unexpected.to_string(), true, false),
            (StatusCode::OK, missing_platforms.to_string(), true, false),
            (StatusCode::NOT_FOUND, "missing".into(), false, false),
            (StatusCode::OK, "invalid JSON".into(), false, false),
        ] {
            let calls = Arc::new(AtomicUsize::new(0));
            let counted = calls.clone();
            let mock = Router::new().route(
                "/latest",
                get(move |headers: HeaderMap| {
                    counted.fetch_add(1, Ordering::Relaxed);
                    assert_eq!(headers[header::USER_AGENT], "Resonance-Website");
                    let body = body.clone();
                    async move { (status, [(header::CONTENT_TYPE, "application/json")], body) }
                }),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let mut cache = ReleaseCache::new().unwrap();
            cache.endpoint = format!("http://{}/latest", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                axum::serve(listener, mock).await.unwrap();
            });
            let data = tempfile::tempdir().unwrap();
            let app = router_with_releases(
                Config {
                    data: data.path().to_owned(),
                    public_url: "http://localhost".into(),
                    registration_open: false,
                    disk_reserve_bytes: 0,
                    trusted_proxies: vec![],
                },
                cache,
            )
            .unwrap();
            let home = app
                .clone()
                .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(home.status(), StatusCode::OK);
            let html = String::from_utf8(
                home.into_body()
                    .collect()
                    .await
                    .unwrap()
                    .to_bytes()
                    .to_vec(),
            )
            .unwrap();
            assert!(html.contains(if has_release {
                "v9.8.7"
            } else {
                "Release details are unavailable"
            }));
            for (platform, ext) in PLATFORMS {
                let response = app
                    .clone()
                    .oneshot(
                        Request::builder()
                            .uri(format!("/download/{platform}"))
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::SEE_OTHER);
                assert_eq!(
                    response.headers()[header::LOCATION],
                    if has_downloads {
                        format!("{REPO}/releases/download/v9.8.7/resonance-v9.8.7-{platform}.{ext}")
                    } else {
                        format!("{REPO}/releases/latest")
                    }
                );
            }
            assert_eq!(calls.load(Ordering::Relaxed), 1);
            assert_eq!(
                app.oneshot(
                    Request::builder()
                        .uri("/download/unknown")
                        .body(Body::empty())
                        .unwrap()
                )
                .await
                .unwrap()
                .status(),
                StatusCode::NOT_FOUND
            );
            server.abort();
        }
    }
}
