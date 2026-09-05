use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Map, Value};

use crate::error::{Error, Result};

const BASE64_SLACK: usize = 4_096;

pub fn decode_file_bytes(
    value: &Value,
    maximum_bytes: usize,
    allow_empty: bool,
) -> Result<Vec<u8>> {
    let object = value.as_object().ok_or(Error::InvalidGitHubResponse)?;
    let kind = string_field(object, "type", 16)?;
    let encoding = string_field(object, "encoding", 16)?;
    let content = string_field(
        object,
        "content",
        maximum_bytes.saturating_mul(2) + BASE64_SLACK,
    )?;
    let size = object
        .get("size")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or(Error::InvalidGitHubResponse)?;
    if kind != "file"
        || encoding != "base64"
        || size > maximum_bytes
        || (!allow_empty && size == 0)
        || content.len() > maximum_bytes.div_ceil(3).saturating_mul(4) + BASE64_SLACK
    {
        return Err(Error::InvalidGitHubResponse);
    }
    let compact = content.replace('\n', "");
    let bytes = STANDARD
        .decode(compact.as_bytes())
        .map_err(|_| Error::InvalidGitHubResponse)?;
    if bytes.len() != size || bytes.len() > maximum_bytes || STANDARD.encode(&bytes) != compact {
        return Err(Error::InvalidGitHubResponse);
    }
    Ok(bytes)
}

pub fn decode_json_object(value: &Value, maximum_bytes: usize) -> Result<Map<String, Value>> {
    let bytes = decode_file_bytes(value, maximum_bytes, false)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| Error::InvalidGitHubResponse)?;
    serde_json::from_str::<Map<String, Value>>(text).map_err(|_| Error::InvalidGitHubResponse)
}

fn string_field<'a>(object: &'a Map<String, Value>, name: &str, maximum: usize) -> Result<&'a str> {
    let value = object
        .get(name)
        .and_then(Value::as_str)
        .ok_or(Error::InvalidGitHubResponse)?;
    if value.len() <= maximum {
        Ok(value)
    } else {
        Err(Error::InvalidGitHubResponse)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn accepts_canonical_bounded_github_file() {
        let value = json!({
            "type": "file",
            "encoding": "base64",
            "size": 13,
            "content": "eyJ2ZXJzaW9uIjoxfQ=="
        });
        let object = decode_json_object(&value, 128).unwrap();
        assert_eq!(object.get("version"), Some(&json!(1)));
    }

    #[test]
    fn rejects_non_canonical_corrupt_or_oversized_content() {
        let wrong_size = json!({
            "type": "file", "encoding": "base64", "size": 1, "content": "e30="
        });
        assert!(decode_file_bytes(&wrong_size, 128, false).is_err());

        let non_canonical = json!({
            "type": "file", "encoding": "base64", "size": 2, "content": "e30"
        });
        assert!(decode_file_bytes(&non_canonical, 128, false).is_err());

        let oversized = json!({
            "type": "file", "encoding": "base64", "size": 3, "content": "e30="
        });
        assert!(decode_file_bytes(&oversized, 2, false).is_err());
    }

    #[test]
    fn rejects_arrays_and_invalid_utf8() {
        let array = json!({
            "type": "file", "encoding": "base64", "size": 3, "content": "WzFd"
        });
        assert!(decode_json_object(&array, 128).is_err());

        let invalid_utf8 = json!({
            "type": "file", "encoding": "base64", "size": 1, "content": "/w=="
        });
        assert!(decode_json_object(&invalid_utf8, 128).is_err());
    }
}
