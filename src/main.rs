mod id;
mod gateway;

use std::net::SocketAddr;

use axum::{routing::get, Router, extract::State, Json};
use tracing_subscriber::{fmt, EnvFilter};
use gateway::ws_web_route;
mod config;
mod meta;

#[tokio::main]
async fn main() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().with_env_filter(env_filter).init();

    let cfg = config::Config::from_env();

    let (online_tx, online_rx) = tokio::sync::watch::channel::<usize>(0);
    let meta_backend: std::sync::Arc<dyn meta::MetaStore> = std::sync::Arc::new(meta::MemoryMetaStore::new());

    let state = gateway::AppState {
        ping_interval: cfg.ping_interval,
        meta: meta_backend,
        online_tx,
        online_rx,
        origin_whitelist: cfg.allowed_origins.clone(),
    };

    // 打印运行时环境配置，便于排障
    log_runtime_env(&cfg);

    // 仅在线人数，移除房间清理与日统计

    let app = Router::new()
        .route("/ws", get(ws_web_route))
        .route("/v1/ws", get(ws_web_route))
        .route("/v1/ws/web", get(ws_web_route))
        .route("/web", get(ws_web_route))
        .route("/v1/metrics/online", get(get_online))
        .with_state(state);

    let addr: SocketAddr = ([0,0,0,0], cfg.port).into();
    tracing::info!(%addr, "listening");
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind port");
    axum::serve(listener, app).await.expect("server error");
}

fn log_runtime_env(cfg: &config::Config) {
    use tracing::info;
    let allowed = cfg
        .allowed_origins
        .as_ref()
        .map(|s| {
            let mut v: Vec<_> = s.iter().cloned().collect();
            v.sort();
            v.join(",")
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "<empty>".to_string());
    info!(port = cfg.port, ping_interval_secs = cfg.ping_interval.map(|d| d.as_secs()), allowed_origins = %allowed, "startup config");
}


#[derive(serde::Serialize)]
struct OnlineCount { online: usize }

async fn get_online(State(state): State<gateway::AppState>) -> Json<OnlineCount> {
    Json(OnlineCount { online: *state.online_rx.borrow() })
}
