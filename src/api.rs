use std::cmp::Reverse;

use axum::{extract::{Query, State}, Json, http::HeaderMap};
use serde::Serialize;

use crate::{time, gateway::AppState};
use crate::events::{BusinessEvent, format_message, UpdatePresencePayload};

#[derive(Debug, Serialize)]
pub struct TopRoom {
    pub room: String,
    pub count: usize,
    pub path: String,
    pub title: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct ActiveQuery { pub limit: Option<usize> }

pub async fn top_active_rooms(State(state): State<AppState>, Query(q): Query<ActiveQuery>) -> Json<Vec<TopRoom>> {
    let limit = q.limit.unwrap_or(10).min(100);
    let now = time::now();
    let mut list = state.rooms.snapshot_counts(now, state.ttl).await;
    // sort by count desc, then by room asc for stability
    list.sort_by_key(|(name, c)| (Reverse(*c), name.clone()));
    let mut out = Vec::with_capacity(list.len().min(limit));
    for (room, count) in list.into_iter().take(limit) {
        // 没有额外元数据来源，暂以 room 作为 path/title 占位，便于前端展示
        out.push(TopRoom { room: room.clone(), count, path: room.clone(), title: room });
    }
    Json(out)
}

#[derive(Debug, serde::Deserialize)]
pub struct GetPresenceQuery { pub room_name: String }

#[derive(Debug, Serialize)]
pub struct PresenceView { pub identity: String, pub joined_at: Option<u64>, pub updated_at: u64 }

pub async fn get_room_presence(State(state): State<AppState>, Query(q): Query<GetPresenceQuery>) -> Json<Vec<PresenceView>> {
    let list = state.meta.room_presence(&q.room_name).await;
    let mut out = Vec::with_capacity(list.len());
    for m in list {
        let joined_at = m.room_joined_at.get(&q.room_name).cloned();
        out.push(PresenceView { identity: m.identity, joined_at, updated_at: m.updated_at_ms });
    }
    Json(out)
}

#[derive(Debug, serde::Deserialize)]
pub struct PresenceUpdateBody {
    pub room_name: String,
    #[serde(default)] pub display_name: Option<String>,
    #[serde(default)] pub position: Option<u32>,
}

pub async fn update_presence(State(state): State<AppState>, headers: HeaderMap, Json(body): Json<PresenceUpdateBody>) -> Json<&'static str> {
    if let Some(sess) = headers.get("x-socket-session-id").and_then(|v| v.to_str().ok()) {
        let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
        state.meta.touch_by_session(sess, now_ms).await;
        if let Some(meta) = state.meta.find_by_session(sess).await {
            let room = state.rooms.get_or_create(&body.room_name);
            let payload = format_message(
                BusinessEvent::ActivityUpdatePresence,
                UpdatePresencePayload {
                    identity: meta.identity,
                    room_name: body.room_name.clone(),
                    updated_at: now_ms,
                    display_name: body.display_name.clone(),
                    position: body.position,
                }
            );
            room.broadcast_event(payload);
        }
    }
    Json("ok")
}

#[derive(Debug, Serialize)]
pub struct RoomsInfo { pub rooms: Vec<String>, pub room_count: std::collections::HashMap<String, usize> }

pub async fn get_rooms_info(State(state): State<AppState>) -> Json<RoomsInfo> {
    let now = time::now();
    let list = state.rooms.snapshot_counts(now, state.ttl).await;
    let mut map = std::collections::HashMap::new();
    let mut rooms = Vec::new();
    for (name, count) in list {
        map.insert(name.clone(), count);
        rooms.push(name);
    }
    Json(RoomsInfo { rooms, room_count: map })
}

#[derive(Debug, serde::Serialize)]
pub struct OnlineTodayResp { pub date: String, pub max: usize, pub total: usize, pub backend: &'static str }

pub async fn get_online_today(State(state): State<AppState>) -> Json<OnlineTodayResp> {
    let day = chrono::Local::now().format("%Y-%m-%d").to_string();
    if let Some((max, total)) = state.meta.online_stats_today().await {
        Json(OnlineTodayResp { date: day, max, total, backend: "redis" })
    } else {
        // fallback for memory backend
        let cur = state.meta.unique_session_count().await;
        Json(OnlineTodayResp { date: day, max: cur, total: 0, backend: "memory" })
    }
}
