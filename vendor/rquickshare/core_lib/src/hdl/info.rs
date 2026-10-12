use super::TextPayloadType;
use crate::utils::RemoteDeviceInfo;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TransferMetadata {
    pub id: String,
    pub source: Option<RemoteDeviceInfo>,
    pub pin_code: Option<String>,
    pub destination: Option<String>,
    pub files: Option<Vec<String>>,
    pub text_type: Option<TextPayloadType>,
    pub text_description: Option<String>,
    pub text_payload: Option<String>,
    pub total_bytes: u64,
    pub ack_bytes: u64,
}
