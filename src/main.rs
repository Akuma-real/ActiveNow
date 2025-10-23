mod id;
mod time;
mod presence;
mod ws;

use std::{env, net::SocketAddr, time::Duration, collections::HashSet};

use axum::{routing::get, Router};
use presence::Rooms;
use tracing_subscriber::{fmt, EnvFilter};
use ws::ws_route;

fn read_env_u64(key: &str, default: u64) -> u64 {
    env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn read_origin_whitelist() -> Option<HashSet<String>> {
    let raw = env::var("ALLOWED_ORIGINS").unwrap_or_default();
    let items: Vec<_> = raw.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).map(|s| s.to_string()).collect();
    if items.is_empty() { None } else { Some(items.into_iter().collect()) }
}

#[tokio::main]
async fn main() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().with_env_filter(env_filter).init();

    let port = env::var("PORT").ok().and_then(|v| v.parse::<u16>().ok()).unwrap_or(8080);
    let ttl_secs = read_env_u64("PRESENCE_TTL", 30);
    let ping_secs = read_env_u64("PING_INTERVAL", 0);

    let state = ws::AppState {
        rooms: Rooms::new(),
        ttl: Duration::from_secs(ttl_secs),
        ping_interval: if ping_secs > 0 { Some(Duration::from_secs(ping_secs)) } else { None },
        origin_whitelist: read_origin_whitelist(),
    };

    // spawn cleanup task
    let rooms_for_cleanup = state.rooms.clone();
    let ttl_for_cleanup = state.ttl;
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        loop {
            ticker.tick().await;
            rooms_for_cleanup.cleanup_all(std::time::Instant::now(), ttl_for_cleanup).await;
        }
    });

    let app = Router::new()
        .route("/v1/ws", get(ws_route))
        .with_state(state);

    let addr: SocketAddr = ([0,0,0,0], port).into();
    tracing::info!(%addr, "listening");
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind port");
    axum::serve(listener, app).await.expect("server error");
}
