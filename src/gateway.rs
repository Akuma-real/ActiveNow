use std::{collections::HashSet, time::Duration};

use axum::{extract::{Query, State, ws::{WebSocket, WebSocketUpgrade, Message}}, response::IntoResponse, http::{HeaderMap, StatusCode}};
use futures_util::{StreamExt, SinkExt};
use serde::{Deserialize, Serialize};

use tokio::sync::{watch, broadcast};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::events::{BusinessEvent, format_message, VisitorOnlinePayload, LeavePresencePayload, VisitorOfflinePayload, JoinPresencePayload};
use crate::{id::new_sid, presence::Rooms, time};
use crate::meta::MetaStore;

#[derive(Clone)]
/// 全局共享应用状态（网关所需）
pub struct AppState {
    pub rooms: Rooms,
    pub ttl: Duration,
    pub ping_interval: Option<Duration>,
    pub origin_whitelist: Option<HashSet<String>>,
    pub meta: std::sync::Arc<dyn MetaStore>,
    pub online_tx: watch::Sender<usize>,
    pub online_rx: watch::Receiver<usize>,
    pub web_event_tx: broadcast::Sender<String>,
}

#[derive(Debug, Deserialize)]
pub struct WsQuery { pub room: String, pub socket_session_id: Option<String> }

#[derive(Debug, Deserialize)]
pub struct WebQuery { pub socket_session_id: Option<String> }

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum InMsg { Hb, #[serde(rename_all = "camelCase")] UpdateSid { session_id: String } }

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum OutMsg<'a> {
    Hello { sid: &'a str, ttl: u64, count: usize },
    Sync { count: usize },
}

// 统一事件格式由 events 模块提供

pub async fn ws_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    // room name validation
    if !is_valid_room(&query.room) {
        return (StatusCode::BAD_REQUEST, "invalid room").into_response();
    }

    // origin whitelist (optional, relaxed matching)
    if let Some(whitelist) = &state.origin_whitelist {
        if !whitelist.is_empty() {
            if !origin_allowed(&headers, whitelist) {
                return (StatusCode::FORBIDDEN, "origin not allowed").into_response();
            }
        }
    }

    let room = query.room.clone();
    let sess = extract_session_id(&headers, query.socket_session_id.as_deref());
    ws.on_upgrade(move |socket| handle_ws(socket, state, room, sess))
}

fn origin_allowed(headers: &HeaderMap, whitelist: &std::collections::HashSet<String>) -> bool {
    use url::Url;

    // '*' means allow all
    if whitelist.iter().any(|s| s.trim() == "*") { return true; }

    let orig_raw = match headers.get("origin").and_then(|v| v.to_str().ok()) {
        Some(v) if !v.trim().is_empty() => v.trim(),
        _ => return false, // 更安全：无 Origin 时拒绝（可用 '*' 放宽）
    };

    // 规范化 Origin：scheme, host(lowercase), explicit port(with known default)
    let parsed = match Url::parse(orig_raw) { Ok(u) => u, Err(_) => return false };
    let scheme = parsed.scheme().to_ascii_lowercase();
    let host = match parsed.host_str() { Some(h) => h.to_ascii_lowercase(), None => return false };
    let port = parsed.port_or_known_default();
    let canonical = match port {
        Some(p) => format!("{}://{}:{}", scheme, host, p),
        None => format!("{}://{}", scheme, host),
    };

    // 逐项匹配（宽松）：
    // 1) 完整 origin 精确匹配（大小写规范化 + 默认端口显式化）
    // 2) host[:port] 匹配（列表项只写 host 则忽略端口；写了端口则端口需一致）
    // 3) 通配域名：以 '*.domain' 或 '.domain' 表示子域/后缀匹配
    // 4) 列表项若本身是一个 URL，则按规范化后与 canonical 比较
    for item in whitelist.iter() {
        let e = item.trim().trim_end_matches('/').to_ascii_lowercase();
        if e.is_empty() { continue; }

        // 4) URL 形式
        if e.starts_with("http://") || e.starts_with("https://") {
            if let Ok(u) = url::Url::parse(&e) {
                let hs = u.host_str().map(|h| h.to_ascii_lowercase());
                let ps = u.port_or_known_default();
                let sc = u.scheme().to_ascii_lowercase();
                if let Some(hs) = hs {
                    let cand = match ps { Some(p) => format!("{}://{}:{}", sc, hs, p), None => format!("{}://{}", sc, hs) };
                    if cand == canonical { return true; }
                }
            }
            continue;
        }

        // 3) 后缀/通配符域名
        if let Some(suffix) = e.strip_prefix("*.").or_else(|| e.strip_prefix('.')) {
            let suffix = suffix.trim_start_matches('.');
            if host == suffix || host.ends_with(&format!(".{}", suffix)) { return true; }
            continue;
        }

        // 2) host[:port]
        if let Some((eh, ep)) = e.split_once(':') {
            if eh == host {
                if let Ok(ep) = ep.parse::<u16>() {
                    if let Some(p) = port { if p == ep { return true; } }
                }
            }
            continue;
        }

        // 2) 仅 host（忽略端口）
        if e == host { return true; }

        // 1) 回退比较：完整字符串（考虑到少数 Origin 可能无端口）
        if e == canonical { return true; }
    }

    false
}

async fn handle_ws(mut ws: WebSocket, state: AppState, room_name: String, session_id: Option<String>) {
    let room = state.rooms.get_or_create(&room_name);
    let sid = new_sid();

    // Join and get current effective count
    let now = time::now();
    let _count = room.join(&sid, now, state.ttl).await;

    // metadata + online
    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
    let sess_id = session_id.clone().unwrap_or_else(|| sid.clone());
    state.meta.upsert_identity(&sid, sess_id.clone(), now_ms).await;
    state.meta.join_room(&sid, &room_name, now_ms).await;
    // broadcast join
    let join_evt = format_message(
        BusinessEvent::ActivityJoinPresence,
        JoinPresencePayload { identity: sid.clone(), room_name: room_name.clone(), joined_at: now_ms }
    );
    room.broadcast_event(join_evt);
    let count = state.meta.unique_session_count().await;
    let _ = state.online_tx.send(count);

    // Send hello
    let hello = serde_json::to_string(&OutMsg::Hello {
        sid: &sid,
        ttl: time::as_secs_u64(state.ttl),
        count: _count,
    }).unwrap_or_else(|_| "{}".to_string());
    if ws.send(Message::Text(hello.into())).await.is_err() { return; }

    // Subscribe to updates
    let mut rx = room.subscribe();
    let mut ev_rx = room.subscribe_events();
    // 拆分读写，便于 select 处理
    let (mut tx, mut rx_ws) = ws.split();

    let mut ping_interval = state.ping_interval.map(tokio::time::interval);

    loop {
        tokio::select! {
            msg = rx_ws.next() => {
                match msg {
                    Some(Ok(Message::Text(t))) => {
                        if let Ok(InMsg::Hb) = serde_json::from_str::<InMsg>(&t) {
                            room.hb(&sid, time::now()).await;
                        } else if let Ok(InMsg::UpdateSid { session_id }) = serde_json::from_str::<InMsg>(&t) {
                            state.meta.set_session_id(&sid, session_id, now_ms).await;
                            let count = state.meta.unique_session_count().await;
                            let _ = state.online_tx.send(count);
                        }
                    }
                    Some(Ok(Message::Binary(_))) => { /* ignore */ }
                    Some(Ok(Message::Ping(_))) => { /* tungstenite handles pong automatically */ }
                    Some(Ok(Message::Pong(_))) => { /* ignore */ }
                    Some(Ok(Message::Close(_))) => break,
                    Some(Err(_e)) => break,
                    None => break,
                }
            }
            changed = rx.changed() => {
                if changed.is_ok() {
                    let count = *rx.borrow();
                    let sync = serde_json::to_string(&OutMsg::Sync { count }).unwrap_or_else(|_| "{}".to_string());
                    if tx.send(Message::Text(sync.into())).await.is_err() { break; }
                } else { break; }
            }
            evt = ev_rx.recv() => {
                match evt {
                    Ok(payload) => { if tx.send(Message::Text(payload.into())).await.is_err() { break; } }
                    Err(_) => break,
                }
            }
            _ = async {
                if let Some(interval) = &mut ping_interval { interval.tick().await; true } else { tokio::task::yield_now().await; false }
            }, if ping_interval.is_some() => {
                if tx.send(Message::Ping(Vec::new().into())).await.is_err() { break; }
            }
        }
    }

    // leave
    let _ = room.leave(&sid, time::now(), state.ttl).await;
    // broadcast leave (统一格式)
    let leave_evt = format_message(
        BusinessEvent::ActivityLeavePresence,
        LeavePresencePayload { identity: sid.clone(), room_name: room_name.clone() }
    );
    room.broadcast_event(leave_evt);
    state.meta.leave_room(&sid, &room_name, now_ms).await;
    state.meta.clear(&sid).await;
    let count = state.meta.unique_session_count().await;
    let _ = state.online_tx.send(count);
}

fn is_valid_room(room: &str) -> bool {
    if room.is_empty() || room.len() > 256 { return false; }
    room.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | ':' | '@' | '-'))
}

fn extract_session_id(headers: &HeaderMap, query_sid: Option<&str>) -> Option<String> {
    if let Some(v) = headers.get("x-socket-session-id").and_then(|v| v.to_str().ok()) {
        if !v.is_empty() { return Some(v.to_string()); }
    }
    query_sid.map(|s| s.to_string())
}

pub async fn ws_web_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<WebQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
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

    // send connect ack
    let connect = format_message(BusinessEvent::GatewayConnect, "WebSocket 已连接");
    if ws.send(Message::Text(connect.into())).await.is_err() { return; }

    // subscribe online + broadcasted events
    let mut rx = state.online_rx.clone();
    let mut ev_rx = state.web_event_tx.subscribe();
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
                    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
                    let payload = format_message(
                        BusinessEvent::VisitorOnline,
                        VisitorOnlinePayload { online: *rx.borrow(), timestamp: ts }
                    );
                    if tx.send(Message::Text(payload.into())).await.is_err() { break; }
                } else { break; }
            }
            evt = ev_rx.recv() => {
                match evt {
                    Ok(payload) => { if tx.send(Message::Text(payload.into())).await.is_err() { break; } }
                    Err(_) => break,
                }
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
    // broadcast VISITOR_OFFLINE to all web listeners
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
    let offline = format_message(
        BusinessEvent::VisitorOffline,
        VisitorOfflinePayload { online: count, timestamp: ts, session_id: sess_id }
    );
    let _ = state.web_event_tx.send(offline);
}
