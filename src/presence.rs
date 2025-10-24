use std::{collections::HashMap, sync::Arc};

use dashmap::DashMap;
use tokio::sync::{RwLock, watch};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct Rooms {
    inner: Arc<DashMap<String, Arc<Room>>>,
}

impl Rooms {
    pub fn new() -> Self {
        Self { inner: Arc::new(DashMap::new()) }
    }

    pub fn get_or_create(&self, name: &str) -> Arc<Room> {
        if let Some(r) = self.inner.get(name) { return r.clone(); }
        let room = Arc::new(Room::new());
        let entry = self.inner.entry(name.to_string()).or_insert_with(|| room.clone());
        entry.clone()
    }

    pub async fn cleanup_all(&self, now: Instant, ttl: Duration) {
        // 先快照，避免在 DashMap 借用持有期间 await
        let rooms: Vec<(String, Arc<Room>)> = self.inner
            .iter()
            .map(|e| (e.key().clone(), e.value().clone()))
            .collect();

        let mut empty_keys = Vec::new();
        for (key, room) in rooms {
            let count = room.cleanup(now, ttl).await;
            if count == 0 { empty_keys.push(key); }
        }
        for k in empty_keys { let _ = self.inner.remove(&k); }
    }

    pub async fn snapshot_counts(&self, now: Instant, ttl: Duration) -> Vec<(String, usize)> {
        let rooms: Vec<(String, Arc<Room>)> = self.inner
            .iter()
            .map(|e| (e.key().clone(), e.value().clone()))
            .collect();

        let mut out = Vec::with_capacity(rooms.len());
        for (name, room) in rooms {
            let c = room.active_count(now, ttl).await;
            if c > 0 { out.push((name, c)); }
        }
        out
    }
}

pub struct Room {
    members: RwLock<HashMap<String, Instant>>, // sid -> last_seen
    count_tx: watch::Sender<usize>,
    count_rx: watch::Receiver<usize>,
}

impl Room {
    pub fn new() -> Self {
        let (tx, rx) = watch::channel(0usize);
        Self {
            members: RwLock::new(HashMap::new()),
            count_tx: tx,
            count_rx: rx,
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<usize> { self.count_rx.clone() }

    pub async fn join(&self, sid: &str, now: Instant, ttl: Duration) -> usize {
        {
            let mut members = self.members.write().await;
            members.insert(sid.to_string(), now);
        }
        self.update_count_if_changed(now, ttl).await
    }

    pub async fn hb(&self, sid: &str, now: Instant) {
        let mut members = self.members.write().await;
        if let Some(ent) = members.get_mut(sid) {
            *ent = now;
        }
    }

    pub async fn leave(&self, sid: &str, now: Instant, ttl: Duration) -> usize {
        {
            let mut members = self.members.write().await;
            members.remove(sid);
        }
        self.update_count_if_changed(now, ttl).await
    }

    // 当前未对外暴露直接读取接口，watch 订阅已满足使用场景。

    // 如需同步快照，可按需提供专用只读方法；当前未使用，移除阻塞版本以保持纯异步。

    pub async fn cleanup(&self, now: Instant, ttl: Duration) -> usize {
        let changed = {
            let mut members = self.members.write().await;
            let before = members.len();
            members.retain(|_, &mut last| now.duration_since(last) < ttl);
            members.len() != before
        };
        let new_count = self.effective_count(now, ttl).await;
        if changed { self.send_count_if_diff(new_count); }
        new_count
    }

    async fn update_count_if_changed(&self, now: Instant, ttl: Duration) -> usize {
        let count = self.effective_count(now, ttl).await;
        self.send_count_if_diff(count);
        count
    }

    fn send_count_if_diff(&self, count: usize) {
        let last = *self.count_rx.borrow();
        if last != count {
            let _ = self.count_tx.send(count);
        }
    }

    async fn effective_count(&self, now: Instant, ttl: Duration) -> usize {
        let members = self.members.read().await;
        members.values().filter(|&&t| now.duration_since(t) < ttl).count()
    }

    pub async fn active_count(&self, now: Instant, ttl: Duration) -> usize {
        self.effective_count(now, ttl).await
    }
}
