use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const SNAPSHOT: usize = 256 * 1024;
pub const CHUNK: usize = 64 * 1024;
pub const MAX_OFFSET: u64 = 9_007_199_254_740_991;
pub const NODES: usize = 10_000;
pub const PAGE: usize = 500;
pub const BRANCHES: usize = 64;
pub const BODY: usize = 8192;
#[derive(Debug)]
pub struct Error {
    pub code: &'static str,
    pub message: String,
    pub status: u16,
}
impl Error {
    pub fn new(code: &'static str, message: impl Into<String>, status: u16) -> Self {
        Self {
            code,
            message: message.into(),
            status,
        }
    }
    pub fn json(&self) -> serde_json::Value {
        json!({"code":self.code,"message":self.message})
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::NotFound => {
                Self::new("NOT_FOUND", "Fichier ou dossier disparu.", 404)
            }
            std::io::ErrorKind::PermissionDenied => {
                Self::new("PERMISSION", "Permission de lecture refusée.", 403)
            }
            _ => Self::new("INTERNAL", "Lecture du disque impossible.", 500),
        }
    }
}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (
            StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            Json(self.json()),
        )
            .into_response()
    }
}
pub type Result<T> = std::result::Result<T, Error>;
pub fn offset(value: &str) -> Result<u64> {
    if value.is_empty() || value.len() > 20 || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::new(
            "CURSOR_INVALID",
            "Position d’octets invalide.",
            409,
        ));
    }
    value
        .parse::<u64>()
        .ok()
        .filter(|n| *n <= MAX_OFFSET)
        .ok_or_else(|| {
            Error::new(
                "CURSOR_INVALID",
                "Position supérieure à 2^53−1 octets.",
                409,
            )
        })
}
pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Directory,
    File,
}
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RootDto {
    pub root_id: String,
    pub node_id: String,
    pub absolute_path: String,
    pub managed_reason: Option<String>,
}
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub relative_path: String,
    pub kind: Kind,
    pub size: String,
    pub identity: String,
    #[serde(rename = "allocatedSize")]
    pub allocated_size: Option<String>,
    pub state: String,
    pub eligible: bool,
    pub reason: String,
}
#[derive(Serialize, Deserialize, Debug)]
pub struct EntryPage {
    pub entries: Vec<Entry>,
    pub revision: String,
    pub cursor: Option<String>,
}
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Chunk {
    pub generation: String,
    pub start: String,
    pub end: String,
    pub size: String,
    pub data: String,
    pub partial_start: bool,
}
#[derive(Default, Deserialize)]
pub struct ContentOptions {
    pub generation: Option<String>,
    pub before: Option<String>,
    pub after: Option<String>,
    #[serde(skip)]
    pub limit: Option<usize>,
}
#[derive(Clone, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub root_id: String,
    pub file_id: String,
    pub generation: String,
    pub offset: String,
    pub revision: u64,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ClientMessage {
    Subscribe {
        #[serde(flatten)]
        selection: Selection,
    },
    Ack {
        revision: u64,
        generation: String,
        end: String,
    },
    Branches {
        #[serde(rename = "rootId")]
        root_id: String,
        ids: Vec<String>,
    },
    Unsubscribe,
}
