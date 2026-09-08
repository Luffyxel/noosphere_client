use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProvisioningConsent {
    pub version: u8,
    pub create_public_repository: bool,
    pub restrict_installation_to_repository: bool,
}

impl ProvisioningConsent {
    pub fn validate(&self) -> bool {
        self.version == 1
            && self.create_public_repository
            && self.restrict_installation_to_repository
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositorySummary {
    pub id: u64,
    pub name: String,
    pub url: String,
    pub created: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitHubViewer {
    pub id: u64,
    pub login: String,
    pub avatar_url: String,
    pub name: Option<String>,
    pub repository: RepositorySummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NoosphereUser {
    pub id: u64,
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: String,
    pub repository: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LookupUser {
    pub id: u64,
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: String,
    pub repository: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum UserLookup {
    Missing {
        found: bool,
    },
    Found {
        found: bool,
        registered: bool,
        user: LookupUser,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Conversation {
    pub version: u8,
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub created_at: String,
    pub peer: NoosphereUser,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FriendRequest {
    pub version: u8,
    pub id: String,
    pub direction: String,
    pub user: NoosphereUser,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SocialState {
    pub conversations: Vec<Conversation>,
    pub incoming: Vec<FriendRequest>,
    pub outgoing: Vec<FriendRequest>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct WakeSignals {
    pub conversation_ids: Vec<String>,
    pub social_changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Message {
    pub version: u8,
    pub id: String,
    pub conversation_id: String,
    pub sent_at: String,
    pub text: String,
    pub sender_id: u64,
    pub own: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RealtimeSignal {
    pub version: u8,
    pub kind: String,
    pub session_id: String,
    pub created_at: String,
    pub expires_at: String,
    pub sdp: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Published {
    pub published: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedOut {
    pub signed_out: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FriendRemoved {
    pub removed: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FriendRequestDeclined {
    pub declined: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationResult {
    pub shown: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmokeConfig {
    pub enabled: bool,
    pub synthetic_media: bool,
    pub instance_profile_slot: Option<u8>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SmokeResult {
    pub version: u8,
    pub native_bridge: bool,
    pub social_bridge: bool,
    pub web_rtc_data_channel: bool,
    pub web_rtc_media: bool,
    pub media_permission: bool,
    pub brand_assets: bool,
    pub diagnostics: Vec<String>,
    pub instance_profile_slot: u8,
}
