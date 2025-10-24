use std::cmp::Reverse;

use axum::{extract::{Query, State}, Json};
use serde::Serialize;

use crate::{time, ws::AppState};

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

