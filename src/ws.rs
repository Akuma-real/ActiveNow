use std::{collections::HashSet, time::Duration};

use axum::{extract::{Query, State, ws::{WebSocket, WebSocketUpgrade, Message}}, response::IntoResponse, http::{HeaderMap, StatusCode}};
use futures_util::{StreamExt, SinkExt};
use serde::{Deserialize, Serialize};

use crate::{id::new_sid, presence::{Rooms}, time};

#[derive(Clone)]
pub struct AppState {
    pub rooms: Rooms,
    pub ttl: Duration,
    pub ping_interval: Option<Duration>,
    pub origin_whitelist: Option<HashSet<String>>,
}

#[derive(Debug, Deserialize)]
pub struct WsQuery { pub room: String }

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum InMsg { Hb }

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum OutMsg<'a> {
    Hello { sid: &'a str, ttl: u64, count: usize },
    Sync { count: usize },
}

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

    // origin whitelist (optional)
    if let Some(whitelist) = &state.origin_whitelist {
        if !whitelist.is_empty() {
            let origin_ok = headers.get("origin").and_then(|v| v.to_str().ok())
                .map(|o| whitelist.contains(o)).unwrap_or(false);
            if !origin_ok {
                return (StatusCode::FORBIDDEN, "origin not allowed").into_response();
            }
        }
    }

    let room = query.room.clone();
    ws.on_upgrade(move |socket| handle_ws(socket, state, room))
}

async fn handle_ws(mut ws: WebSocket, state: AppState, room_name: String) {
    let room = state.rooms.get_or_create(&room_name);
    let sid = new_sid();

    // Join and get current effective count
    let now = time::now();
    let count = room.join(&sid, now, state.ttl).await;

    // Send hello
    let hello = serde_json::to_string(&OutMsg::Hello {
        sid: &sid,
        ttl: time::as_secs_u64(state.ttl),
        count,
    }).unwrap_or_else(|_| "{}".to_string());
    if ws.send(Message::Text(hello)).await.is_err() { return; }

    // Subscribe to count updates
    let mut rx = room.subscribe();
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
                    if tx.send(Message::Text(sync)).await.is_err() { break; }
                } else { break; }
            }
            _ = async {
                if let Some(interval) = &mut ping_interval { interval.tick().await; true } else { tokio::task::yield_now().await; false }
            }, if ping_interval.is_some() => {
                if tx.send(Message::Ping(Vec::new())).await.is_err() { break; }
            }
        }
    }

    // leave
    let _ = room.leave(&sid, time::now(), state.ttl).await;
}

fn is_valid_room(room: &str) -> bool {
    if room.is_empty() || room.len() > 256 { return false; }
    room.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | ':' | '@' | '-'))
}
