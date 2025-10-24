use async_trait::async_trait;
use dashmap::DashMap;
use redis::{aio::ConnectionManager, AsyncCommands};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SocketMetadata {
    pub identity: String,
    pub session_id: String,
    pub connected_at_ms: u64,
    pub updated_at_ms: u64,
    pub room_joined_at: HashMap<String, u64>,
}

#[async_trait]
pub trait MetaStore: Send + Sync {
    async fn upsert_identity(&self, sid: &str, session_id: String, now_ms: u64);
    async fn set_session_id(&self, sid: &str, session_id: String, now_ms: u64);
    async fn join_room(&self, sid: &str, room: &str, now_ms: u64);
    async fn leave_room(&self, sid: &str, room: &str, now_ms: u64);
    async fn clear(&self, sid: &str);
    async fn unique_session_count(&self) -> usize;
    async fn touch_by_session(&self, session_id: &str, now_ms: u64);
    async fn room_presence(&self, room: &str) -> Vec<SocketMetadata>;
    async fn update_online_stats(&self, _online: usize) {}
    async fn find_by_session(&self, session_id: &str) -> Option<SocketMetadata>;
    async fn online_stats_today(&self) -> Option<(usize, usize)> { None }
}

// ---------------------- Memory backend ----------------------

#[derive(Clone, Default)]
pub struct MemoryMetaStore {
    inner: DashMap<String, SocketMetadata>,
}

impl MemoryMetaStore { pub fn new() -> Self { Self::default() } }

#[async_trait]
impl MetaStore for MemoryMetaStore {
    async fn upsert_identity(&self, sid: &str, session_id: String, now_ms: u64) {
        self.inner
            .entry(sid.to_string())
            .and_modify(|m| { m.session_id = session_id.clone(); m.updated_at_ms = now_ms; })
            .or_insert_with(|| SocketMetadata { identity: sid.to_string(), session_id, connected_at_ms: now_ms, updated_at_ms: now_ms, room_joined_at: HashMap::new() });
    }
    async fn set_session_id(&self, sid: &str, session_id: String, now_ms: u64) {
        if let Some(mut ent) = self.inner.get_mut(sid) { ent.session_id = session_id; ent.updated_at_ms = now_ms; }
    }
    async fn join_room(&self, sid: &str, room: &str, now_ms: u64) {
        if let Some(mut ent) = self.inner.get_mut(sid) { ent.room_joined_at.insert(room.to_string(), now_ms); ent.updated_at_ms = now_ms; }
    }
    async fn leave_room(&self, sid: &str, room: &str, now_ms: u64) {
        if let Some(mut ent) = self.inner.get_mut(sid) { ent.room_joined_at.remove(room); ent.updated_at_ms = now_ms; }
    }
    async fn clear(&self, sid: &str) { self.inner.remove(sid); }
    async fn unique_session_count(&self) -> usize {
        use std::collections::HashSet; let mut set = HashSet::new(); for v in self.inner.iter() { set.insert(v.session_id.clone()); } set.len()
    }
    async fn touch_by_session(&self, session_id: &str, now_ms: u64) {
        for mut item in self.inner.iter_mut() { if item.session_id == session_id { item.updated_at_ms = now_ms; } }
    }
    async fn room_presence(&self, room: &str) -> Vec<SocketMetadata> {
        let mut out = Vec::new(); for v in self.inner.iter() { if v.room_joined_at.contains_key(room) { out.push(v.clone()); } } out
    }
    async fn find_by_session(&self, session_id: &str) -> Option<SocketMetadata> {
        for v in self.inner.iter() { if v.session_id == session_id { return Some(v.clone()); } }
        None
    }
    async fn online_stats_today(&self) -> Option<(usize, usize)> {
        None
    }
}

// ---------------------- Redis backend ----------------------

#[derive(Clone)]
pub struct RedisMetaStore { conn: ConnectionManager }

impl RedisMetaStore {
    pub async fn new(url: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let client = redis::Client::open(url)?;
        // redis >=0.30: get_connection_manager (tokio-comp)
        let conn = client.get_connection_manager().await?;
        Ok(Self { conn })
    }
}

const KEY_SOCKET: &str = "socket";
const KEY_MAX_ONLINE: &str = "max_online_count";
const KEY_MAX_ONLINE_TOTAL: &str = "max_online_count:total";

fn to_json(v: &SocketMetadata) -> String { serde_json::to_string(v).unwrap_or_else(|_| "{}".into()) }
fn from_json(s: &str) -> Option<SocketMetadata> { serde_json::from_str(s).ok() }
fn today() -> String { chrono::Local::now().format("%Y-%m-%d").to_string() }

#[async_trait]
impl MetaStore for RedisMetaStore {
    async fn upsert_identity(&self, sid: &str, session_id: String, now_ms: u64) {
        let mut conn = self.conn.clone();
        let existing: Option<String> = conn.hget(KEY_SOCKET, sid).await.unwrap_or(None);
        let mut meta = existing.and_then(|s| from_json(&s)).unwrap_or_else(|| SocketMetadata { identity: sid.to_string(), session_id: session_id.clone(), connected_at_ms: now_ms, updated_at_ms: now_ms, room_joined_at: HashMap::new() });
        meta.session_id = session_id; meta.updated_at_ms = now_ms;
        let _ : () = conn.hset(KEY_SOCKET, sid, to_json(&meta)).await.unwrap_or(());
    }
    async fn set_session_id(&self, sid: &str, session_id: String, now_ms: u64) {
        let mut conn = self.conn.clone();
        if let Ok(Some(s)) = conn.hget::<_,_,Option<String>>(KEY_SOCKET, sid).await { if let Some(mut m) = from_json(&s) { m.session_id = session_id; m.updated_at_ms = now_ms; let _: () = conn.hset(KEY_SOCKET, sid, to_json(&m)).await.unwrap_or(()); } }
    }
    async fn join_room(&self, sid: &str, room: &str, now_ms: u64) {
        let mut conn = self.conn.clone();
        if let Ok(Some(s)) = conn.hget::<_,_,Option<String>>(KEY_SOCKET, sid).await { if let Some(mut m) = from_json(&s) { m.room_joined_at.insert(room.to_string(), now_ms); m.updated_at_ms = now_ms; let _: () = conn.hset(KEY_SOCKET, sid, to_json(&m)).await.unwrap_or(()); } }
    }
    async fn leave_room(&self, sid: &str, room: &str, now_ms: u64) {
        let mut conn = self.conn.clone();
        if let Ok(Some(s)) = conn.hget::<_,_,Option<String>>(KEY_SOCKET, sid).await { if let Some(mut m) = from_json(&s) { m.room_joined_at.remove(room); m.updated_at_ms = now_ms; let _: () = conn.hset(KEY_SOCKET, sid, to_json(&m)).await.unwrap_or(()); } }
    }
    async fn clear(&self, sid: &str) {
        let mut conn = self.conn.clone(); let _: () = conn.hdel(KEY_SOCKET, sid).await.unwrap_or(());
    }
    async fn unique_session_count(&self) -> usize {
        let mut conn = self.conn.clone();
        let m: HashMap<String, String> = conn.hgetall(KEY_SOCKET).await.unwrap_or_default();
        let mut set = std::collections::HashSet::new(); for (_, v) in m { if let Some(meta) = from_json(&v) { set.insert(meta.session_id); } } set.len()
    }
    async fn touch_by_session(&self, session_id: &str, now_ms: u64) {
        let mut conn = self.conn.clone();
        let m: HashMap<String, String> = conn.hgetall(KEY_SOCKET).await.unwrap_or_default();
        for (k, v) in m { if let Some(mut meta) = from_json(&v) { if meta.session_id == session_id { meta.updated_at_ms = now_ms; let _: () = conn.hset(KEY_SOCKET, k, to_json(&meta)).await.unwrap_or(()); } } }
    }
    async fn room_presence(&self, room: &str) -> Vec<SocketMetadata> {
        let mut conn = self.conn.clone();
        let m: HashMap<String, String> = conn.hgetall(KEY_SOCKET).await.unwrap_or_default();
        let mut out = Vec::new(); for (_, v) in m { if let Some(meta) = from_json(&v) { if meta.room_joined_at.contains_key(room) { out.push(meta); } } } 
        out
    }
    async fn update_online_stats(&self, online: usize) {
        let mut conn = self.conn.clone(); let day = today();
        // max
        if let Ok(Some(cur)) = conn.hget::<_,_,Option<usize>>(KEY_MAX_ONLINE, &day).await { if online > cur { let _: () = conn.hset(KEY_MAX_ONLINE, &day, online).await.unwrap_or(()); } } else { let _: () = conn.hset(KEY_MAX_ONLINE, &day, online).await.unwrap_or(()); }
        // total +1
        let _: i64 = conn.hincr(KEY_MAX_ONLINE_TOTAL, &day, 1).await.unwrap_or(0);
    }
    async fn find_by_session(&self, session_id: &str) -> Option<SocketMetadata> {
        let mut conn = self.conn.clone();
        let m: HashMap<String, String> = conn.hgetall(KEY_SOCKET).await.unwrap_or_default();
        for (_, v) in m { if let Some(meta) = from_json(&v) { if meta.session_id == session_id { return Some(meta); } } }
        None
    }
    async fn online_stats_today(&self) -> Option<(usize, usize)> {
        let mut conn = self.conn.clone();
        let day = today();
        let max: Option<usize> = conn.hget(KEY_MAX_ONLINE, &day).await.ok();
        let total: Option<usize> = conn.hget(KEY_MAX_ONLINE_TOTAL, &day).await.ok();
        match (max, total) { (Some(m), Some(t)) => Some((m, t)), _ => None }
    }
}
