//! The Ferraria server binary: generates the world (seed from `WORLD_SEED`,
//! default 42), then serves the game on `PORT` (default 3000) — `/ws` for
//! gameplay, `/healthz` + `/api/status` for monitoring, `web/` statics for
//! the wasm client.

use std::time::Instant;

const DEFAULT_WORLD_SEED: u64 = 42;

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
    let seed: u64 = std::env::var("WORLD_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_WORLD_SEED);
    let web_dir = std::env::var("WEB_DIR").unwrap_or_else(|_| "web".into());

    tracing::info!(seed, "generating world...");
    let start = Instant::now();
    let world = ferraria_server::worldgen::generate(seed);
    tracing::info!(
        seed,
        width = world.width,
        height = world.height,
        spawn = ?world.spawn,
        elapsed_ms = start.elapsed().as_millis() as u64,
        "world generated"
    );

    let app = ferraria_server::net::router(world, seed, &web_dir);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("ferraria-server listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
