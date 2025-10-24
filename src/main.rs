mod id;
mod time;
mod presence;
mod gateway;
mod api;
mod events;

use std::{net::SocketAddr, time::Duration};

use axum::{routing::{get, post}, Router};
use presence::Rooms;
use tracing_subscriber::{fmt, EnvFilter};
use gateway::{ws_route, ws_web_route};
mod config;
mod meta;

#[tokio::main]
async fn main() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().with_env_filter(env_filter).init();

    let cfg = config::Config::from_env();

    let (online_tx, online_rx) = tokio::sync::watch::channel::<usize>(0);
    let (web_event_tx, _web_event_rx) = tokio::sync::broadcast::channel::<String>(256);

    // 选择 MetaStore 后端：Redis 或内存
    let meta_backend: std::sync::Arc<dyn meta::MetaStore> = if let Some(url) = &cfg.redis_url {
        match meta::RedisMetaStore::new(url).await {
            Ok(store) => std::sync::Arc::new(store),
            Err(e) => {
                panic!("Redis init failed: {}", e);
            }
        }
    } else {
        std::sync::Arc::new(meta::MemoryMetaStore::new())
    };

    let state = gateway::AppState {
        rooms: Rooms::new(),
        ttl: cfg.ttl,
        ping_interval: cfg.ping_interval,
        origin_whitelist: cfg.allowed_origins,
        meta: meta_backend,
        online_tx,
        online_rx,
        web_event_tx,
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

    // 后台去抖：每秒批量写入在线统计到 Redis
    {
        let mut rx = state.online_rx.clone();
        let meta = state.meta.clone();
        tokio::spawn(async move {
            let mut latest = *rx.borrow();
            let mut dirty = false;
            let mut ticker = tokio::time::interval(Duration::from_secs(1));
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        if dirty {
                            meta.update_online_stats(latest).await;
                            dirty = false;
                        }
                    }
                    changed = rx.changed() => {
                        if changed.is_err() { break; }
                        latest = *rx.borrow();
                        dirty = true;
                    }
                }
            }
        });
    }

    let app = Router::new()
        .route("/v1/ws", get(ws_route))
        .route("/v1/ws/web", get(ws_web_route))
        .route("/web", get(ws_web_route))
        .route("/v1/rooms/active", get(api::top_active_rooms))
        .route("/v1/activity/presence", get(api::get_room_presence))
        .route("/v1/activity/presence/update", post(api::update_presence))
        .route("/v1/activity/rooms", get(api::get_rooms_info))
        .route("/v1/metrics/online/today", get(api::get_online_today))
        .with_state(state);

    let addr: SocketAddr = ([0,0,0,0], cfg.port).into();
    tracing::info!(%addr, "listening");
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind port");
    axum::serve(listener, app).await.expect("server error");
}
