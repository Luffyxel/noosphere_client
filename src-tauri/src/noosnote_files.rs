use std::collections::{BTreeMap, BTreeSet};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::Method;
use secrecy::{ExposeSecret as _, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha1::Sha1;
use sha2::{Digest as _, Sha256};
use tauri::State;
use uuid::Uuid;

use crate::{
    commands,
    error::{Error, Result},
    noosnote,
    state::AppState,
};

const MAX_BYTES: usize = 20 * 1024 * 1024;
const CHUNK_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    id: String,
    name: String,
    media_type: String,
    size: usize,
    checksum: String,
    chunks: Vec<Chunk>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Chunk {
    sha: String,
    size: usize,
}

fn hex_hash(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

pub fn validate(value: &Value) -> Result<Attachment> {
    let file: Attachment = serde_json::from_value(value.clone()).map_err(|_| Error::InvalidData)?;
    if !Uuid::parse_str(&file.id).is_ok_and(|id| id.to_string() == file.id)
        || file.name.is_empty()
        || file.name.len() > 255
        || file.name.contains(['/', '\\', '\0', '\r', '\n'])
        || matches!(file.name.as_str(), "." | "..")
        || file.media_type.is_empty()
        || file.media_type.len() > 100
        || !file.media_type.is_ascii()
        || file.size == 0
        || file.size > MAX_BYTES
        || !hex_hash(&file.checksum, 64)
        || file.chunks.len() != file.size.div_ceil(CHUNK_BYTES)
    {
        return Err(Error::InvalidData);
    }
    for (index, chunk) in file.chunks.iter().enumerate() {
        if !hex_hash(&chunk.sha, 40)
            || chunk.size != (file.size - index * CHUNK_BYTES).min(CHUNK_BYTES)
        {
            return Err(Error::InvalidData);
        }
    }
    Ok(file)
}

fn chunk_path(workspace: &str, attachment: &str, index: usize) -> String {
    format!("notes/{workspace}/attachments/{attachment}/{index:04}.bin")
}

fn chunk_key(owner: u64, attachment: &str, index: usize) -> String {
    format!("nn-file-{owner}-{attachment}-{index}")
}

fn blob_hash(bytes: &[u8]) -> String {
    let mut hash = Sha1::new();
    hash.update(format!("blob {}\0", bytes.len()).as_bytes());
    hash.update(bytes);
    hex::encode(hash.finalize())
}

fn media_type(name: &str, bytes: &[u8]) -> Result<&'static str> {
    let extension = name
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "pdf" if bytes.starts_with(b"%PDF-") => Ok("application/pdf"),
        "docx" if bytes.starts_with(b"PK\x03\x04") => {
            Ok("application/vnd.openxmlformats-officedocument.wordprocessingml.document")
        }
        "doc" if bytes.starts_with(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]) => {
            Ok("application/msword")
        }
        "pdf" | "docx" | "doc" => Err(Error::InvalidData),
        _ => Ok("application/octet-stream"),
    }
}

pub(crate) fn cache_file(
    store: &crate::secure_blob::SecureBlobStore,
    owner: u64,
    name: String,
    content: String,
) -> Result<Attachment> {
    if content.len() > MAX_BYTES.div_ceil(3) * 4 {
        return Err(Error::ResponseTooLarge);
    }
    let bytes = STANDARD.decode(&content).map_err(|_| Error::InvalidData)?;
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(Error::ResponseTooLarge);
    }
    let file = Attachment {
        id: Uuid::new_v4().to_string(),
        media_type: media_type(&name, &bytes)?.into(),
        name,
        size: bytes.len(),
        checksum: hex::encode(Sha256::digest(&bytes)),
        chunks: bytes
            .chunks(CHUNK_BYTES)
            .map(|part| Chunk {
                sha: blob_hash(part),
                size: part.len(),
            })
            .collect(),
    };
    validate(&serde_json::to_value(&file).map_err(|_| Error::Local)?)?;
    for (index, part) in bytes.chunks(CHUNK_BYTES).enumerate() {
        if let Err(error) = store.save(
            &chunk_key(owner, &file.id, index),
            SecretString::from(STANDARD.encode(part)),
        ) {
            for written in 0..index {
                let _ = store.remove(&chunk_key(owner, &file.id, written));
            }
            return Err(error);
        }
    }
    Ok(file)
}

#[tauri::command]
pub async fn noosnote_import_file(
    state: State<'_, AppState>,
    workspace_id: String,
    name: String,
    content: String,
) -> std::result::Result<Attachment, String> {
    async fn import(
        state: &AppState,
        workspace: &str,
        name: String,
        content: String,
    ) -> Result<Attachment> {
        let viewer = commands::session_viewer(state).await?;
        if !Uuid::parse_str(workspace).is_ok_and(|id| id.to_string() == workspace)
            || content.len() > MAX_BYTES.div_ceil(3) * 4
        {
            return Err(Error::ResponseTooLarge);
        }
        let store = state.blobs.clone();
        tokio::task::spawn_blocking(move || cache_file(&store, viewer.id, name, content))
            .await
            .map_err(|_| Error::Local)?
    }
    import(&state, &workspace_id, name, content)
        .await
        .map_err(|error| error.to_string())
}

pub async fn publish_chunks(
    state: &AppState,
    root: &str,
    token: &SecretString,
    pages: &[Value],
    remote: &BTreeMap<String, String>,
) -> Result<Vec<Value>> {
    let owner = commands::session_viewer(state).await?.id;
    let mut updates = Vec::new();
    let mut seen = BTreeSet::new();
    let mut available: BTreeSet<String> = remote.values().cloned().collect();
    for page in pages {
        let Some(value) = page.get("attachment") else {
            continue;
        };
        let file = validate(value)?;
        let workspace = page
            .get("workspaceId")
            .and_then(Value::as_str)
            .ok_or(Error::InvalidData)?;
        for (index, part) in file.chunks.iter().enumerate() {
            let path = chunk_path(workspace, &file.id, index);
            if !seen.insert(path.clone()) || remote.get(&path) == Some(&part.sha) {
                continue;
            }
            if available.contains(&part.sha) {
                updates.push(json!({"path":path,"mode":"100644","type":"blob","sha":part.sha}));
                continue;
            }
            let store = state.blobs.clone();
            let key = chunk_key(owner, &file.id, index);
            let expected = part.clone();
            let content = tokio::task::spawn_blocking(move || {
                let encoded = store.load(&key)?.ok_or(Error::NoosnotePageUnavailable)?;
                let bytes = STANDARD
                    .decode(encoded.expose_secret())
                    .map_err(|_| Error::InvalidData)?;
                if bytes.len() != expected.size || blob_hash(&bytes) != expected.sha {
                    return Err(Error::InvalidData);
                }
                Ok(encoded)
            })
            .await
            .map_err(|_| Error::Local)??;
            let response = state
                .github
                .api_json::<Value>(
                    Method::POST,
                    &format!("{root}/git/blobs"),
                    Some(token.expose_secret()),
                    None,
                    Some(&json!({"encoding":"base64","content":content.expose_secret()})),
                    &[],
                )
                .await?;
            if response
                .data
                .as_ref()
                .and_then(|value| value.get("sha"))
                .and_then(Value::as_str)
                != Some(&part.sha)
            {
                return Err(Error::InvalidGitHubResponse);
            }
            available.insert(part.sha.clone());
            updates.push(json!({"path":path,"mode":"100644","type":"blob","sha":part.sha}));
        }
    }
    Ok(updates)
}

#[tauri::command]
pub async fn noosnote_read_file(
    state: State<'_, AppState>,
    workspace_id: String,
    page_id: String,
) -> std::result::Result<String, String> {
    async fn read(state: &AppState, workspace: &str, page_id: &str) -> Result<String> {
        let viewer = commands::session_viewer(state).await?;
        let snapshot = noosnote::load_page(state, workspace, page_id).await?;
        let page = snapshot
            .files
            .get(&format!("notes/{workspace}/pages/{page_id}.json"))
            .ok_or(Error::NoosnotePageUnavailable)?;
        let file = validate(page.get("attachment").ok_or(Error::InvalidData)?)?;
        let mut bytes = Vec::with_capacity(file.size);
        let mut remote_checked = false;
        for (index, part) in file.chunks.iter().enumerate() {
            let key = chunk_key(viewer.id, &file.id, index);
            let store = state.blobs.clone();
            let load_key = key.clone();
            let cached = tokio::task::spawn_blocking(move || store.load(&load_key))
                .await
                .map_err(|_| Error::Local)??;
            let chunk = if let Some(encoded) = cached {
                STANDARD
                    .decode(encoded.expose_secret())
                    .map_err(|_| Error::InvalidData)?
            } else {
                let token = state.access_token().await?;
                let root = format!(
                    "/repos/{}/{}",
                    viewer.login,
                    noosnote::repository_name(viewer.id)
                );
                if !remote_checked {
                    let response = state
                        .github
                        .api_json::<Value>(
                            Method::GET,
                            &root,
                            Some(token.expose_secret()),
                            None,
                            None,
                            &[],
                        )
                        .await?;
                    noosnote::normalize_repository(
                        response.data.as_ref().ok_or(Error::InvalidGitHubResponse)?,
                        &viewer,
                    )?;
                    remote_checked = true;
                }
                let response = state
                    .github
                    .api_json::<Value>(
                        Method::GET,
                        &format!("{root}/git/blobs/{}", part.sha),
                        Some(token.expose_secret()),
                        None,
                        None,
                        &[],
                    )
                    .await?;
                let mut value = response.data.ok_or(Error::InvalidGitHubResponse)?;
                if value.get("sha").and_then(Value::as_str) != Some(&part.sha) {
                    return Err(Error::InvalidGitHubResponse);
                }
                value["type"] = Value::String("file".into());
                let chunk =
                    crate::repository_content::decode_file_bytes(&value, CHUNK_BYTES, false)?;
                if chunk.len() != part.size || blob_hash(&chunk) != part.sha {
                    return Err(Error::InvalidGitHubResponse);
                }
                let store = state.blobs.clone();
                let encoded = SecretString::from(STANDARD.encode(&chunk));
                tokio::task::spawn_blocking(move || store.save(&key, encoded))
                    .await
                    .map_err(|_| Error::Local)??;
                chunk
            };
            if chunk.len() != part.size || blob_hash(&chunk) != part.sha {
                return Err(Error::InvalidData);
            }
            bytes.extend_from_slice(&chunk);
        }
        if hex::encode(Sha256::digest(&bytes)) != file.checksum {
            return Err(Error::InvalidData);
        }
        if commands::session_viewer(state).await?.id != viewer.id {
            return Err(Error::SessionExpired);
        }
        Ok(STANDARD.encode(&bytes))
    }
    read(&state, &workspace_id, &page_id)
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_file_chunks_survive_a_restart_without_crossing_accounts() {
        let directory = tempfile::tempdir().unwrap();
        let store = crate::secure_blob::SecureBlobStore::new(directory.path()).unwrap();
        let mut bytes = vec![b'a'; CHUNK_BYTES + 128];
        bytes[..9].copy_from_slice(b"%PDF-1.7\n");
        let file = cache_file(&store, 42, "cours.pdf".into(), STANDARD.encode(&bytes)).unwrap();
        assert_eq!(file.chunks.len(), 2);
        drop(store);
        let restored = crate::secure_blob::SecureBlobStore::new(directory.path()).unwrap();
        let mut loaded = Vec::new();
        for (index, part) in file.chunks.iter().enumerate() {
            assert!(
                restored
                    .load(&chunk_key(99, &file.id, index))
                    .unwrap()
                    .is_none()
            );
            let secret = restored
                .load(&chunk_key(42, &file.id, index))
                .unwrap()
                .unwrap();
            let chunk = STANDARD.decode(secret.expose_secret()).unwrap();
            assert_eq!(chunk.len(), part.size);
            assert_eq!(blob_hash(&chunk), part.sha);
            loaded.extend(chunk);
        }
        assert_eq!(loaded, bytes);
        assert_eq!(hex::encode(Sha256::digest(&loaded)), file.checksum);
    }

    #[test]
    fn chunk_ids_match_git_and_paths_stay_in_the_workspace() {
        assert_eq!(
            blob_hash(b"hello\n"),
            "ce013625030ba8dba906f756967f9e9ca394464a"
        );
        assert_eq!(
            chunk_path("workspace", "file", 2),
            "notes/workspace/attachments/file/0002.bin"
        );
    }

    #[test]
    fn metadata_rejects_wrong_chunk_sizes_and_unsafe_names() {
        let value = json!({"id":Uuid::new_v4().to_string(),"name":"cours.pdf","mediaType":"application/pdf","size":6,"checksum":hex::encode(Sha256::digest(b"hello\n")),"chunks":[{"sha":blob_hash(b"hello\n"),"size":6}]});
        assert!(validate(&value).is_ok());
        let mut wrong = value.clone();
        wrong["name"] = json!("../cours.pdf");
        assert!(validate(&wrong).is_err());
        let mut wrong = value.clone();
        wrong["chunks"][0]["size"] = json!(5);
        assert!(validate(&wrong).is_err());
        let mut wrong = value;
        wrong["chunks"][0]["sha"] = json!("invalid");
        assert!(validate(&wrong).is_err());
    }
}
