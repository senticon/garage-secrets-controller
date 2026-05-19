use chrono::{DateTime, Utc};
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DesiredState {
    Requested,
    Ready,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketRecord {
    pub name: String,
    #[serde(default)]
    pub state: Option<DesiredState>,
    #[serde(default)]
    pub garage_bucket_id: Option<String>,
    #[serde(default)]
    pub error_message: Option<String>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRecord {
    pub name: String,
    #[serde(default)]
    pub access_key_id: Option<String>,
    #[serde(default)]
    pub secret_access_key: Option<String>,
    #[serde(default)]
    pub state: Option<DesiredState>,
    #[serde(default)]
    pub error_message: Option<String>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrantRecord {
    pub key: String,
    pub bucket: String,
    #[serde(deserialize_with = "de_boolish")]
    pub read: bool,
    #[serde(deserialize_with = "de_boolish")]
    pub write: bool,
    #[serde(deserialize_with = "de_boolish")]
    pub owner: bool,
    #[serde(default)]
    pub state: Option<DesiredState>,
    #[serde(default)]
    pub error_message: Option<String>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

pub fn is_requested(state: &Option<DesiredState>) -> bool {
    matches!(
        state,
        None | Some(DesiredState::Requested) | Some(DesiredState::Error)
    )
}

fn de_boolish<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(deserializer)?;
    match v {
        serde_json::Value::Bool(b) => Ok(b),
        serde_json::Value::String(s) => match s.to_ascii_lowercase().as_str() {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(de::Error::custom(format!("invalid boolean string '{s}'"))),
        },
        _ => Err(de::Error::custom("expected bool or boolean string")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_requested_state() {
        let raw = r#"{"name":"b","state":"requested"}"#;
        let rec: BucketRecord = serde_json::from_str(raw).expect("valid bucket json");
        assert_eq!(rec.state, Some(DesiredState::Requested));
    }

    #[test]
    fn parse_grant_flags() {
        let raw = r#"{"key":"k","bucket":"b","read":true,"write":false,"owner":false,"state":"requested"}"#;
        let rec: GrantRecord = serde_json::from_str(raw).expect("valid grant json");
        assert!(rec.read);
        assert!(!rec.write);
    }
}
