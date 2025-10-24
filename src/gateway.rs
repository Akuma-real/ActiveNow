use std::{collections::HashSet, time::Duration};

use axum::{extract::{Query, State, ws::{WebSocket, WebSocketUpgrade, Message}}, response::IntoResponse, http::HeaderMap};
use futures_util::{StreamExt, SinkExt};
use serde::{Deserialize, Serialize};

use tokio::sync::watch;
use std::time::{SystemTime, UNIX_EPOCH};
use crate::id::new_sid;
use crate::meta::MetaStore;

#[derive(Clone)]
/// 全局共享应用状态（仅在线人数）
pub struct AppState {
    pub ping_interval: Option<Duration>,
    pub meta: std::sync::Arc<dyn MetaStore>,
    pub online_tx: watch::Sender<usize>,
    pub online_rx: watch::Receiver<usize>,
    pub origin_whitelist: Option<HashSet<String>>,
}

#[derive(Debug, Deserialize)]
pub struct WebQuery { pub socket_session_id: Option<String> }

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum InMsg { #[serde(rename_all = "camelCase")] UpdateSid { session_id: String } }

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum OutMsg<'a> {
    Sync { count: usize },
    Hello { sid: &'a str, count: usize },
}

fn extract_session_id(headers: &HeaderMap, query_sid: Option<&str>) -> Option<String> {
    if let Some(v) = headers.get("x-socket-session-id").and_then(|v| v.to_str().ok()) {
        if !v.is_empty() { return Some(v.to_string()); }
    }
    query_sid.map(|s| s.to_string())
}

fn origin_allowed(headers: &HeaderMap, whitelist: &HashSet<String>) -> bool {
    if whitelist.iter().any(|s| s.trim() == "*") { return true; }
    let origin = match headers.get("origin").and_then(|v| v.to_str().ok()) {
        Some(v) if !v.trim().is_empty() => v.trim().to_ascii_lowercase(),
        _ => return false,
    };
    let origin_norm = origin.trim_end_matches('/');
    let (host, port) = parse_host_port(origin_norm);
    for item in whitelist.iter() {
        let e = item.trim().trim_end_matches('/');
        if e.is_empty() { continue; }
        if e.starts_with("http://") || e.starts_with("https://") {
            if origin_norm == e { return true; }
            continue;
        }
        if let Some(suffix) = e.strip_prefix("*.").or_else(|| e.strip_prefix('.')) {
            let sfx = suffix.trim_start_matches('.');
            if host == sfx || host.ends_with(&format!(".{}", sfx)) { return true; }
            continue;
        }
        if let Some((eh, ep)) = e.split_once(':') {
            if eh == host && Some(ep) == port { return true; }
            continue;
        }
        if e == host { return true; }
    }
    false
}

fn parse_host_port(origin: &str) -> (String, Option<&str>) {
    let after_scheme = origin.splitn(2, "://").nth(1).unwrap_or(origin);
    let authority = after_scheme.split('/').next().unwrap_or(after_scheme);
    let auth = authority.trim_matches(|c| c == '[' || c == ']');
    if let Some(idx) = auth.rfind(':') {
        let (h, p) = auth.split_at(idx);
        (h.to_string(), Some(&p[1..]))
    } else {
        (auth.to_string(), None)
    }
}

pub async fn ws_web_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<WebQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if let Some(whitelist) = &state.origin_whitelist {
        if !whitelist.is_empty() && !origin_allowed(&headers, whitelist) {
            return axum::http::StatusCode::FORBIDDEN.into_response();
        }
    }
    let sess = extract_session_id(&headers, query.socket_session_id.as_deref());
    ws.on_upgrade(move |socket| handle_ws_web(socket, state, sess))
}

async fn handle_ws_web(mut ws: WebSocket, state: AppState, session_id: Option<String>) {
    let sid = new_sid();
    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
    let sess_id = session_id.clone().unwrap_or_else(|| sid.clone());
    state.meta.upsert_identity(&sid, sess_id.clone(), now_ms).await;
    let count = state.meta.unique_session_count().await;
    let _ = state.online_tx.send(count);

    // 首包：hello（当前在线）
    let hello = serde_json::to_string(&OutMsg::Hello { sid: &sid, count }).unwrap_or_else(|_| "{}".to_string());
    if ws.send(Message::Text(hello.into())).await.is_err() { return; }

    // 仅订阅在线人数变化
    let mut rx = state.online_rx.clone();
    let (mut tx, mut rx_ws) = ws.split();
    let mut ping_interval = state.ping_interval.map(tokio::time::interval);

    loop {
        tokio::select! {
            msg = rx_ws.next() => {
                match msg {
                    Some(Ok(Message::Text(t))) => {
                        if let Ok(InMsg::UpdateSid { session_id }) = serde_json::from_str::<InMsg>(&t) {
                            state.meta.set_session_id(&sid, session_id, now_ms).await;
                            let count = state.meta.unique_session_count().await;
                            let _ = state.online_tx.send(count);
                        }
                    }
                    Some(Ok(Message::Close(_))) => break,
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
            changed = rx.changed() => {
                if changed.is_ok() {
                    let payload = serde_json::to_string(&OutMsg::Sync { count: *rx.borrow() }).unwrap_or_else(|_| "{}".to_string());
                    if tx.send(Message::Text(payload.into())).await.is_err() { break; }
                } else { break; }
            }
            _ = async {
                if let Some(interval) = &mut ping_interval { interval.tick().await; true } else { tokio::task::yield_now().await; false }
            }, if ping_interval.is_some() => {
                if tx.send(Message::Ping(Vec::new().into())).await.is_err() { break; }
            }
        }
    }

    state.meta.clear(&sid).await;
    let count = state.meta.unique_session_count().await;
    let _ = state.online_tx.send(count);
}
