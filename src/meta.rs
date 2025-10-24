use async_trait::async_trait;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SocketMetadata {
    pub identity: String,
    pub session_id: String,
}

#[async_trait]
pub trait MetaStore: Send + Sync {
    async fn upsert_identity(&self, sid: &str, session_id: String, now_ms: u64);
    async fn set_session_id(&self, sid: &str, session_id: String, now_ms: u64);
    async fn clear(&self, sid: &str);
    async fn unique_session_count(&self) -> usize;
}

// ---------------------- Memory backend ----------------------

#[derive(Clone, Default)]
pub struct MemoryMetaStore {
    inner: DashMap<String, SocketMetadata>,
}

impl MemoryMetaStore { pub fn new() -> Self { Self::default() } }

#[async_trait]
impl MetaStore for MemoryMetaStore {
    async fn upsert_identity(&self, sid: &str, session_id: String, _now_ms: u64) {
        self.inner
            .entry(sid.to_string())
            .and_modify(|m| { m.session_id = session_id.clone(); })
            .or_insert_with(|| SocketMetadata { identity: sid.to_string(), session_id });
    }
    async fn set_session_id(&self, sid: &str, session_id: String, _now_ms: u64) {
        if let Some(mut ent) = self.inner.get_mut(sid) { ent.session_id = session_id; }
    }
    async fn clear(&self, sid: &str) { self.inner.remove(sid); }
    async fn unique_session_count(&self) -> usize {
        use std::collections::HashSet; let mut set = HashSet::new(); for v in self.inner.iter() { set.insert(v.session_id.clone()); } set.len()
    }
}
