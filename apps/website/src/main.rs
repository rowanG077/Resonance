use resonance_website::web;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "resonance_website=info,tower_http=info".into()),
        )
        .init();
    let config = web::Config::from_env()?;
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() == 2 && args[0] == "create-admin" {
        use std::io::{IsTerminal, Read};
        anyhow::ensure!(
            !std::io::stdin().is_terminal(),
            "Pipe a password from systemd-ask-password into create-admin USERNAME; passwords are never accepted as command-line arguments."
        );
        let mut password = String::new();
        std::io::stdin().take(131).read_to_string(&mut password)?;
        let password = password.strip_suffix('\n').unwrap_or(&password);
        let password = password.strip_suffix('\r').unwrap_or(password);
        web::create_admin(&config, &args[1], password)?;
        println!("Administrator account created for {}.", args[1]);
        return Ok(());
    }
    if args == ["list-accounts"] {
        for (name, admin) in web::list_accounts(&config)? {
            println!("{name}\t{}", if admin { "administrator" } else { "member" });
        }
        return Ok(());
    }
    if !args.is_empty() {
        anyhow::ensure!(
            args.len() == 2 && ["grant-admin", "revoke-admin"].contains(&args[0].as_str()),
            "Usage: resonance-website [list-accounts | create-admin USERNAME | grant-admin USERNAME | revoke-admin USERNAME]"
        );
        web::set_admin(&config, &args[1], args[0] == "grant-admin")?;
        println!(
            "Administrator access {} for {}.",
            if args[0] == "grant-admin" {
                "granted"
            } else {
                "revoked"
            },
            args[1]
        );
        return Ok(());
    }
    let address = std::env::var("RESONANCE_WEB_LISTEN").unwrap_or_else(|_| "127.0.0.1:3000".into());
    let listener = tokio::net::TcpListener::bind(&address).await?;
    let app = web::router(config.clone())?;
    tokio::spawn(web::maintain(config));
    tracing::info!(%address, "Resonance website listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown())
    .await?;
    Ok(())
}

async fn shutdown() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}
