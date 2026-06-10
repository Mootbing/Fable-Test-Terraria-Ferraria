use axum::{routing::get, Router};
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .fallback_service(ServeDir::new(
            std::env::var("WEB_DIR").unwrap_or_else(|_| "web".into()),
        ));

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("ferraria-server listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
