use serde::{Deserialize, Serialize};

/// The plaintext JSON payload inside a `.diary` v1 file (and the export API).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiaryPayload {
    pub format: String,
    pub version: u32,
    pub exported_at: String,
    pub app: String,
    pub timezone: String,
    pub tags: Vec<DiaryTag>,
    pub entries: Vec<DiaryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiaryTag {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiaryEntry {
    pub date: String,
    pub content_md: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}
