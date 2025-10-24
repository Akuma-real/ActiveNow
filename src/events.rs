use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BusinessEvent {
    GatewayConnect,
    VisitorOnline,
    VisitorOffline,
    ActivityJoinPresence,
    ActivityUpdatePresence,
    ActivityLeavePresence,
}

#[derive(Debug, Serialize)]
pub struct GatewayMessage<T: Serialize> {
    #[serde(rename = "type")]
    pub kind: BusinessEvent,
    pub data: T,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<i32>,
}

pub fn format_message<T: Serialize>(kind: BusinessEvent, data: T) -> String {
    serde_json::to_string(&GatewayMessage { kind, data, code: None })
        .unwrap_or_else(|_| "{}".to_string())
}

#[derive(Debug, Serialize)]
pub struct VisitorOnlinePayload {
    pub online: usize,
    pub timestamp: u64,
}

#[derive(Debug, Serialize)]
pub struct VisitorOfflinePayload {
    pub online: usize,
    pub timestamp: u64,
    #[serde(rename = "sessionId")]
    pub session_id: String,
}

#[derive(Debug, Serialize)]
pub struct UpdatePresencePayload {
    pub identity: String,
    #[serde(rename = "roomName")]
    pub room_name: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct LeavePresencePayload {
    pub identity: String,
    #[serde(rename = "roomName")]
    pub room_name: String,
}

#[derive(Debug, Serialize)]
pub struct JoinPresencePayload {
    pub identity: String,
    #[serde(rename = "roomName")]
    pub room_name: String,
    #[serde(rename = "joinedAt")]
    pub joined_at: u64,
}
