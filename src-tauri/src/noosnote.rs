use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::Method;
use secrecy::{ExposeSecret as _, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use tauri::State;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

use crate::{
    DesktopAppHandle, commands,
    error::{Error, Result},
    github::GitHubClient,
    models::GitHubViewer,
    secure_blob::SecureBlobStore,
    state::AppState,
};

pub const DESCRIPTION: &str = "Notes privées Noosphere";
const MAX_FILE_BYTES: usize = 1024 * 1024;
const MAX_FILES: usize = 8192;
const MARKER: &str = ".noosnote.json";

type CommandResult<T> = std::result::Result<T, String>;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Repository {
    id: u64,
    name: String,
    url: String,
    branch: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Conflict {
    id: String,
    path: String,
    local: Option<Arc<Value>>,
    remote: Option<Arc<Value>>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Index {
    version: u8,
    owner_id: u64,
    repository: Option<Repository>,
    head: Option<String>,
    blobs: BTreeMap<String, String>,
    hashes: BTreeMap<String, String>,
    dirty: BTreeSet<String>,
    last_synced_at: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    owner_id: u64,
    path: String,
    local: Option<Arc<Value>>,
    base: Option<Arc<Value>>,
    base_sha: Option<String>,
    conflict: Option<Conflict>,
}

#[derive(Clone)]
pub struct Cache {
    index: Index,
    documents: BTreeMap<String, Document>,
    opened: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    files: BTreeMap<String, Arc<Value>>,
    deleted_paths: Vec<String>,
    incremental: bool,
    repository: Option<Repository>,
    dirty_count: usize,
    dirty_paths: Vec<String>,
    conflicts: Vec<Conflict>,
    last_synced_at: Option<String>,
    setup: &'static str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    path: String,
    value: Option<Value>,
    base: Option<EditBase>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditBase {
    value: Option<Value>,
}

pub fn repository_name(owner_id: u64) -> String {
    format!("noosnote_{owner_id}")
}

fn index_account(owner: u64) -> String {
    format!("noosnote-{owner}-index")
}

fn document_prefix(owner: u64, path: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(owner.to_be_bytes());
    hash.update(path.as_bytes());
    format!("nn-{}-", &hex::encode(hash.finalize())[..24])
}

fn now() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

fn uuid(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}

fn file_kind(path: &str) -> Result<(&str, &str, &str)> {
    let parts: Vec<_> = path.split('/').collect();
    if parts.len() < 3 || parts[0] != "notes" || !uuid(parts[1]) {
        return Err(Error::InvalidData);
    }
    if parts.len() == 3 && parts[2] == "workspace.json" {
        return Ok(("workspace", parts[1], parts[1]));
    }
    if parts.len() == 4
        && matches!(parts[2], "pages" | "databases")
        && let Some(id) = parts[3].strip_suffix(".json")
        && uuid(id)
    {
        return Ok((
            if parts[2] == "pages" {
                "page"
            } else {
                "database"
            },
            parts[1],
            id,
        ));
    }
    Err(Error::InvalidData)
}

fn field<'a>(value: &'a Value, key: &str, maximum: usize) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| value.len() <= maximum)
        .ok_or(Error::InvalidData)
}

fn array<'a>(value: &'a Value, key: &str, maximum: usize) -> Result<&'a [Value]> {
    value
        .get(key)
        .and_then(Value::as_array)
        .filter(|value| value.len() <= maximum)
        .map(Vec::as_slice)
        .ok_or(Error::InvalidData)
}

fn unique_ids(values: &[Value]) -> Result<()> {
    let mut ids = BTreeSet::new();
    for value in values {
        let id = field(value, "id", 36)?;
        if !uuid(id) || !ids.insert(id) {
            return Err(Error::InvalidData);
        }
    }
    Ok(())
}

fn validate_content(node: &Value, depth: usize, remaining: &mut usize) -> Result<()> {
    if depth > 48 || *remaining == 0 {
        return Err(Error::InvalidData);
    }
    *remaining -= 1;
    let kind = field(node, "type", 32)?;
    if !matches!(
        kind,
        "doc"
            | "paragraph"
            | "text"
            | "heading"
            | "hardBreak"
            | "codeBlock"
            | "blockquote"
            | "bulletList"
            | "orderedList"
            | "listItem"
            | "horizontalRule"
            | "taskList"
            | "taskItem"
            | "noosDatabase"
            | "noosPage"
    ) {
        return Err(Error::InvalidData);
    }
    if kind == "text" {
        field(node, "text", MAX_FILE_BYTES)?;
    }
    if let Some(marks) = node.get("marks") {
        let marks = marks
            .as_array()
            .filter(|marks| marks.len() <= 12)
            .ok_or(Error::InvalidData)?;
        for mark in marks {
            let kind = field(mark, "type", 32)?;
            if !matches!(
                kind,
                "bold" | "italic" | "strike" | "underline" | "code" | "link"
            ) {
                return Err(Error::InvalidData);
            }
            if kind == "link" {
                let href = mark
                    .pointer("/attrs/href")
                    .and_then(Value::as_str)
                    .ok_or(Error::InvalidData)?;
                let url = url::Url::parse(href).map_err(|_| Error::InvalidData)?;
                if href.len() > 2048
                    || !matches!(
                        url.scheme(),
                        "https" | "http" | "mailto" | "tel" | "noosnote"
                    )
                {
                    return Err(Error::InvalidData);
                }
            }
        }
    }
    if kind == "noosDatabase"
        && !node
            .pointer("/attrs/databaseId")
            .and_then(Value::as_str)
            .is_some_and(uuid)
    {
        return Err(Error::InvalidData);
    }
    if kind == "noosPage"
        && !node
            .pointer("/attrs/pageId")
            .and_then(Value::as_str)
            .is_some_and(uuid)
    {
        return Err(Error::InvalidData);
    }
    if let Some(content) = node.get("content") {
        for child in content.as_array().ok_or(Error::InvalidData)? {
            validate_content(child, depth + 1, remaining)?;
        }
    }
    Ok(())
}

fn validate_file(path: &str, value: &Value) -> Result<()> {
    let (kind, workspace, id) = file_kind(path)?;
    if serde_json::to_vec(value)
        .map_err(|_| Error::InvalidData)?
        .len()
        + 1
        > MAX_FILE_BYTES
    {
        return Err(Error::ResponseTooLarge);
    }
    if value.get("version").and_then(Value::as_u64) != Some(1)
        || field(value, "kind", 16)? != kind
        || field(value, "id", 36)? != id
    {
        return Err(Error::InvalidData);
    }
    field(value, "title", 512)?;
    match kind {
        "workspace" => {
            field(value, "icon", 32)?;
            let color = field(value, "color", 7)?;
            if color.len() != 7
                || !color.starts_with('#')
                || !color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(Error::InvalidData);
            }
            let pages = array(value, "pages", 4096)?;
            unique_ids(pages)?;
            let ids: BTreeSet<_> = pages
                .iter()
                .map(|page| page["id"].as_str().unwrap_or_default())
                .collect();
            let parents: BTreeMap<_, _> = pages
                .iter()
                .map(|page| {
                    (
                        page["id"].as_str().unwrap_or_default(),
                        page.get("parentId").and_then(Value::as_str),
                    )
                })
                .collect();
            for page in pages {
                field(page, "title", 512)?;
                field(page, "icon", 32)?;
                if let Some(parent) = page.get("parentId").and_then(Value::as_str)
                    && (!ids.contains(parent) || parent == page["id"])
                {
                    return Err(Error::InvalidData);
                }
                if !page
                    .get("parentId")
                    .is_some_and(|parent| parent.is_null() || parent.is_string())
                {
                    return Err(Error::InvalidData);
                }
            }
            let mut verified = BTreeSet::new();
            for id in &ids {
                let mut visited = BTreeSet::new();
                let mut current = Some(*id);
                while let Some(id) = current {
                    if verified.contains(id) {
                        break;
                    }
                    if !visited.insert(id) {
                        return Err(Error::InvalidData);
                    }
                    current = parents.get(id).copied().flatten();
                }
                verified.extend(visited);
            }
        }
        "page" => {
            if field(value, "workspaceId", 36)? != workspace {
                return Err(Error::InvalidData);
            }
            field(value, "icon", 32)?;
            let content = value.get("content").ok_or(Error::InvalidData)?;
            if content["type"] != "doc" {
                return Err(Error::InvalidData);
            }
            validate_content(content, 0, &mut 50_000)?;
        }
        "database" => {
            if field(value, "workspaceId", 36)? != workspace || !uuid(field(value, "pageId", 36)?) {
                return Err(Error::InvalidData);
            }
            let columns = array(value, "columns", 64)?;
            unique_ids(columns)?;
            if columns
                .iter()
                .filter(|column| column["type"] == "title")
                .count()
                != 1
            {
                return Err(Error::InvalidData);
            }
            for column in columns {
                field(column, "name", 128)?;
                let kind = field(column, "type", 32)?;
                if !matches!(
                    kind,
                    "title"
                        | "text"
                        | "number"
                        | "select"
                        | "multiSelect"
                        | "date"
                        | "checkbox"
                        | "url"
                        | "email"
                        | "phone"
                        | "relation"
                        | "formula"
                        | "rollup"
                ) {
                    return Err(Error::InvalidData);
                }
                if let Some(options) = column.get("options") {
                    let options = options
                        .as_array()
                        .filter(|values| values.len() <= 200)
                        .ok_or(Error::InvalidData)?;
                    unique_ids(options)?;
                    for option in options {
                        field(option, "name", 128)?;
                        field(option, "color", 32)?;
                    }
                }
                if kind == "formula" {
                    field(column, "expression", 4096)?;
                }
            }
            let rows = array(value, "rows", 5000)?;
            unique_ids(rows)?;
            for row in rows {
                if !uuid(field(row, "pageId", 36)?) {
                    return Err(Error::InvalidData);
                }
                let values = row
                    .get("values")
                    .and_then(Value::as_object)
                    .ok_or(Error::InvalidData)?;
                for (key, cell) in values {
                    let column = columns
                        .iter()
                        .find(|column| column["id"] == key.as_str())
                        .ok_or(Error::InvalidData)?;
                    if cell.is_null() {
                        continue;
                    }
                    let kind = column["type"].as_str().ok_or(Error::InvalidData)?;
                    let valid = match kind {
                        "number" => cell.as_f64().is_some_and(f64::is_finite),
                        "checkbox" => cell.is_boolean(),
                        "multiSelect" | "relation" => cell.as_array().is_some_and(|values| {
                            values.len() <= 200
                                && values.iter().all(|value| value.as_str().is_some_and(uuid))
                        }),
                        "select" => cell.as_str().is_some_and(uuid),
                        "text" | "url" | "email" | "phone" | "date" => {
                            cell.as_str().is_some_and(|value| value.len() <= 16_384)
                        }
                        _ => false,
                    };
                    if !valid {
                        return Err(Error::InvalidData);
                    }
                }
            }
            let views = array(value, "views", 24)?;
            if views.is_empty() {
                return Err(Error::InvalidData);
            }
            unique_ids(views)?;
            for view in views {
                field(view, "name", 128)?;
                if !matches!(
                    field(view, "layout", 16)?,
                    "table" | "board" | "list" | "gallery" | "calendar"
                ) {
                    return Err(Error::InvalidData);
                }
                let property = |value: &Value| -> Result<()> {
                    value
                        .as_str()
                        .filter(|id| uuid(id) && columns.iter().any(|column| column["id"] == *id))
                        .ok_or(Error::InvalidData)?;
                    Ok(())
                };
                for hidden in array(view, "hidden", 64)? {
                    property(hidden)?;
                }
                for key in ["groupBy", "dateProperty"] {
                    if let Some(value) = view.get(key) {
                        property(value)?;
                    }
                }
                let filters = array(view, "filters", 64)?;
                unique_ids(filters)?;
                for filter in filters {
                    property(filter.get("propertyId").ok_or(Error::InvalidData)?)?;
                    if !matches!(
                        field(filter, "operator", 32)?,
                        "contains"
                            | "notContains"
                            | "equals"
                            | "notEquals"
                            | "greater"
                            | "less"
                            | "empty"
                            | "notEmpty"
                    ) {
                        return Err(Error::InvalidData);
                    }
                    field(filter, "value", 16_384)?;
                }
                for sort in array(view, "sorts", 64)? {
                    property(sort.get("propertyId").ok_or(Error::InvalidData)?)?;
                    if !matches!(field(sort, "direction", 4)?, "asc" | "desc") {
                        return Err(Error::InvalidData);
                    }
                }
            }
        }
        _ => return Err(Error::InvalidData),
    }
    Ok(())
}

impl Cache {
    fn empty(owner_id: u64) -> Self {
        Self {
            index: Index {
                version: 1,
                owner_id,
                repository: None,
                head: None,
                blobs: BTreeMap::new(),
                hashes: BTreeMap::new(),
                dirty: BTreeSet::new(),
                last_synced_at: None,
            },
            documents: BTreeMap::new(),
            opened: Vec::new(),
        }
    }

    fn snapshot(&self, changed: Option<&BTreeSet<String>>, setup: &'static str) -> Snapshot {
        let mut files = BTreeMap::new();
        let mut deleted_paths = Vec::new();
        for (path, document) in &self.documents {
            if path == MARKER || changed.is_some_and(|changed| !changed.contains(path)) {
                continue;
            }
            if let Some(value) = &document.local {
                files.insert(path.clone(), value.clone());
            } else {
                deleted_paths.push(path.clone());
            }
        }
        Snapshot {
            files,
            deleted_paths,
            incremental: changed.is_some(),
            repository: self.index.repository.clone(),
            dirty_count: self.index.dirty.len(),
            dirty_paths: self.index.dirty.iter().cloned().collect(),
            conflicts: self
                .documents
                .values()
                .filter_map(|document| document.conflict.clone())
                .collect(),
            last_synced_at: self.index.last_synced_at.clone(),
            setup,
        }
    }
}

fn load_cache(store: &SecureBlobStore, owner: u64) -> Result<Cache> {
    let Some(secret) = store.load(&index_account(owner))? else {
        return Ok(Cache::empty(owner));
    };
    let index: Index = serde_json::from_str(secret.expose_secret())
        .map_err(|_| Error::SecureStorageUnavailable)?;
    if index.version != 1 || index.owner_id != owner || index.blobs.len() > MAX_FILES {
        return Err(Error::SecureStorageUnavailable);
    }
    let mut documents = BTreeMap::new();
    for (path, key) in &index.blobs {
        if path != MARKER {
            file_kind(path)?;
        }
        let prefix = document_prefix(owner, path);
        if !key.starts_with(&prefix) || !uuid(&key[prefix.len()..]) {
            return Err(Error::SecureStorageUnavailable);
        }
        if file_kind(path).is_ok_and(|(kind, _, _)| kind == "page") && !index.dirty.contains(path) {
            continue;
        }
        let document = load_document(store, owner, path, key)?;
        documents.insert(path.clone(), document);
    }
    Ok(Cache {
        index,
        documents,
        opened: Vec::new(),
    })
}

fn load_document(store: &SecureBlobStore, owner: u64, path: &str, key: &str) -> Result<Document> {
    let secret = store.load(key)?.ok_or(Error::SecureStorageUnavailable)?;
    let document: Document = serde_json::from_str(secret.expose_secret())
        .map_err(|_| Error::SecureStorageUnavailable)?;
    if document.owner_id != owner || document.path != path {
        return Err(Error::SecureStorageUnavailable);
    }
    if path != MARKER
        && let Some(value) = &document.local
    {
        validate_file(path, value)?;
    }
    Ok(document)
}

fn touch_page(cache: &mut Cache, path: &str) {
    cache.opened.retain(|previous| previous != path);
    cache.opened.push(path.to_owned());
    if cache.opened.len() > 24 {
        cache.opened.remove(0);
    }
}

fn evict_pages(cache: &mut Cache) {
    cache.documents.retain(|path, _| {
        !file_kind(path).is_ok_and(|(kind, _, _)| kind == "page")
            || cache.index.dirty.contains(path)
            || cache.opened.contains(path)
    });
}

fn persist_cache(
    store: &SecureBlobStore,
    cache: &mut Cache,
    changed: &BTreeSet<String>,
) -> Result<()> {
    let mut obsolete = Vec::new();
    let mut written = Vec::new();
    let mut index = cache.index.clone();
    let result = (|| {
        for path in changed {
            let document = cache.documents.get(path).ok_or(Error::Local)?;
            let key = format!(
                "{}{}",
                document_prefix(cache.index.owner_id, path),
                Uuid::new_v4()
            );
            store.save(
                &key,
                SecretString::from(serde_json::to_string(document).map_err(|_| Error::Local)?),
            )?;
            written.push(key.clone());
            if let Some(old) = index.blobs.insert(path.clone(), key) {
                obsolete.push(old);
            }
        }
        store.save(
            &index_account(cache.index.owner_id),
            SecretString::from(serde_json::to_string(&index).map_err(|_| Error::Local)?),
        )?;
        Ok::<_, Error>(())
    })();
    if let Err(error) = result {
        for key in written {
            let _ = store.discard(&key);
        }
        return Err(error);
    }
    cache.index = index;
    for old in obsolete {
        let _ = store.discard(&old);
    }
    Ok(())
}

async fn cached(state: &AppState, owner: u64) -> Result<Cache> {
    let mut current = state.notes.lock().await;
    if current
        .as_ref()
        .is_none_or(|cache| cache.index.owner_id != owner)
    {
        let store = state.blobs.clone();
        let loaded = tokio::task::spawn_blocking(move || load_cache(&store, owner))
            .await
            .map_err(|_| Error::Local)??;
        *current = Some(loaded);
    }
    current.clone().ok_or(Error::Local)
}

async fn install_cache(
    state: &AppState,
    cache: &mut Cache,
    changed: &BTreeSet<String>,
) -> Result<()> {
    let store = state.blobs.clone();
    let mut persisted = cache.clone();
    let paths = changed.clone();
    persisted = tokio::task::spawn_blocking(move || {
        persist_cache(&store, &mut persisted, &paths)?;
        Ok::<_, Error>(persisted)
    })
    .await
    .map_err(|_| Error::Local)??;
    *cache = persisted;
    Ok(())
}

fn item_id(value: &Value) -> Option<&str> {
    value
        .get("id")
        .or_else(|| value.pointer("/attrs/id"))
        .and_then(Value::as_str)
}

fn merge_text(base: &str, local: &str, remote: &str) -> Option<String> {
    fn edit(base: &[char], next: &[char]) -> (usize, usize, Vec<char>) {
        let prefix = base
            .iter()
            .zip(next)
            .take_while(|(left, right)| left == right)
            .count();
        let suffix = base[prefix..]
            .iter()
            .rev()
            .zip(next[prefix..].iter().rev())
            .take_while(|(left, right)| left == right)
            .count();
        (
            prefix,
            base.len() - suffix,
            next[prefix..next.len() - suffix].to_vec(),
        )
    }
    let base: Vec<_> = base.chars().collect();
    let mut edits = [
        edit(&base, &local.chars().collect::<Vec<_>>()),
        edit(&base, &remote.chars().collect::<Vec<_>>()),
    ];
    edits.sort_by_key(|edit| edit.0);
    if edits[0].1 > edits[1].0 || edits[0].0 == edits[1].0 {
        return None;
    }
    let mut merged = base;
    for (start, end, replacement) in edits.into_iter().rev() {
        merged.splice(start..end, replacement);
    }
    Some(merged.into_iter().collect())
}

fn merge_value(
    base: Option<&Value>,
    local: Option<&Value>,
    remote: Option<&Value>,
    depth: usize,
) -> std::result::Result<Option<Value>, ()> {
    if local == remote {
        return Ok(local.cloned());
    }
    if local == base {
        return Ok(remote.cloned());
    }
    if remote == base {
        return Ok(local.cloned());
    }
    if depth > 48 {
        return Err(());
    }
    if let (Some(Value::Object(local)), Some(Value::Object(remote))) = (local, remote) {
        let base = base.and_then(Value::as_object);
        let keys: BTreeSet<_> = local
            .keys()
            .chain(remote.keys())
            .chain(base.into_iter().flat_map(|value| value.keys()))
            .collect();
        let mut merged = serde_json::Map::new();
        for key in keys {
            if let Some(value) = merge_value(
                base.and_then(|base| base.get(key)),
                local.get(key),
                remote.get(key),
                depth + 1,
            )? {
                merged.insert(key.clone(), value);
            }
        }
        return Ok(Some(Value::Object(merged)));
    }
    if let (Some(Value::Array(local)), Some(Value::Array(remote))) = (local, remote) {
        let base = base
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if local
            .iter()
            .chain(remote)
            .chain(base)
            .all(|value| item_id(value).is_some())
        {
            let mut ids = BTreeSet::new();
            let order: Vec<_> = local
                .iter()
                .chain(remote)
                .chain(base)
                .filter_map(item_id)
                .filter(|id| ids.insert(*id))
                .collect();
            let mut merged = Vec::new();
            for id in order {
                fn find<'a>(values: &'a [Value], id: &str) -> Option<&'a Value> {
                    values.iter().find(|value| item_id(value) == Some(id))
                }
                if let Some(value) =
                    merge_value(find(base, id), find(local, id), find(remote, id), depth + 1)?
                {
                    merged.push(value);
                }
            }
            return Ok(Some(Value::Array(merged)));
        }
        if local.len() == base.len() && remote.len() == base.len() {
            let mut merged = Vec::new();
            for ((base, local), remote) in base.iter().zip(local).zip(remote) {
                if let Some(value) = merge_value(Some(base), Some(local), Some(remote), depth + 1)?
                {
                    merged.push(value);
                }
            }
            return Ok(Some(Value::Array(merged)));
        }
    }
    if let (Some(Value::String(base)), Some(Value::String(local)), Some(Value::String(remote))) =
        (base, local, remote)
    {
        return merge_text(base, local, remote)
            .map(|value| Some(Value::String(value)))
            .ok_or(());
    }
    Err(())
}

fn merge_remote(
    cache: &mut Cache,
    values: BTreeMap<String, Option<Arc<Value>>>,
    hashes: BTreeMap<String, String>,
    head: String,
) -> Result<BTreeSet<String>> {
    let mut changed = BTreeSet::new();
    for (path, remote) in values {
        if path == MARKER {
            if let Some(remote) = &remote {
                validate_marker(remote, cache.index.owner_id)?;
            } else {
                let local = cache
                    .documents
                    .get(MARKER)
                    .filter(|document| {
                        document.base.is_none() && cache.index.dirty.contains(MARKER)
                    })
                    .and_then(|document| document.local.as_deref())
                    .ok_or(Error::NoosnoteRepositoryMismatch)?;
                validate_marker(local, cache.index.owner_id)?;
            }
        } else if let Some(value) = &remote {
            validate_file(&path, value)?;
        }
        let document = cache
            .documents
            .entry(path.clone())
            .or_insert_with(|| Document {
                owner_id: cache.index.owner_id,
                path: path.clone(),
                local: None,
                base: None,
                base_sha: None,
                conflict: None,
            });
        match merge_value(
            document.base.as_deref(),
            document.local.as_deref(),
            remote.as_deref(),
            0,
        ) {
            Ok(merged) => {
                if path != MARKER
                    && let Some(value) = &merged
                    && validate_file(&path, value).is_err()
                {
                    document.conflict = Some(Conflict {
                        id: Uuid::new_v4().to_string(),
                        path: path.clone(),
                        local: document.local.clone(),
                        remote: remote.clone(),
                    });
                    document.base = remote;
                    document.base_sha = hashes.get(&path).cloned();
                    cache.index.dirty.insert(path.clone());
                    changed.insert(path);
                    continue;
                }
                document.local = merged.map(Arc::new);
                document.base = remote;
                document.base_sha = hashes.get(&path).cloned();
                document.conflict = None;
                if document.local != document.base {
                    cache.index.dirty.insert(path.clone());
                } else {
                    cache.index.dirty.remove(&path);
                }
            }
            Err(()) => {
                document.conflict = Some(Conflict {
                    id: Uuid::new_v4().to_string(),
                    path: path.clone(),
                    local: document.local.clone(),
                    remote: remote.clone(),
                });
                document.base = remote;
                document.base_sha = hashes.get(&path).cloned();
                cache.index.dirty.insert(path.clone());
            }
        }
        changed.insert(path);
    }
    cache.index.hashes = hashes;
    cache.index.head = Some(head);
    cache.index.last_synced_at = Some(now());
    Ok(changed)
}

fn validate_marker(value: &Value, owner: u64) -> Result<()> {
    if value.get("version").and_then(Value::as_u64) != Some(1)
        || value.get("ownerId").and_then(Value::as_u64) != Some(owner)
    {
        return Err(Error::NoosnoteRepositoryMismatch);
    }
    Ok(())
}

fn normalize_repository(value: &Value, viewer: &GitHubViewer) -> Result<Repository> {
    if value.get("private").and_then(Value::as_bool) != Some(true) {
        return Err(Error::NoosnoteRepositoryPublic);
    }
    let name = repository_name(viewer.id);
    if value.pointer("/owner/id").and_then(Value::as_u64) != Some(viewer.id)
        || value.get("name").and_then(Value::as_str) != Some(name.as_str())
    {
        return Err(Error::NoosnoteRepositoryMismatch);
    }
    let branch = field(value, "default_branch", 200)?;
    if branch.is_empty() || branch.contains(['\0', '\n', '?', '#']) {
        return Err(Error::InvalidGitHubResponse);
    }
    Ok(Repository {
        id: value
            .get("id")
            .and_then(Value::as_u64)
            .filter(|id| *id > 0)
            .ok_or(Error::InvalidGitHubResponse)?,
        name: name.clone(),
        url: format!("https://github.com/{}/{name}", viewer.login),
        branch: branch.to_owned(),
    })
}

fn segment(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>()
}

fn sha(value: &Value, key: &str) -> Result<String> {
    let value = field(value, key, 64)?;
    if !matches!(value.len(), 40 | 64) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::InvalidGitHubResponse);
    }
    Ok(value.to_owned())
}

async fn request(
    client: &GitHubClient,
    token: &SecretString,
    method: Method,
    path: &str,
    body: Option<&Value>,
    allowed: &[u16],
) -> Result<crate::github::ApiResponse<Value>> {
    client
        .api_json(
            method,
            path,
            Some(token.expose_secret()),
            None,
            body,
            allowed,
        )
        .await
}

async fn installation(state: &AppState, token: &SecretString, owner: u64) -> Result<u64> {
    let response = request(
        &state.github,
        token,
        Method::GET,
        "/user/installations?per_page=100",
        None,
        &[],
    )
    .await?;
    response
        .data
        .as_ref()
        .and_then(|value| value.get("installations"))
        .and_then(Value::as_array)
        .and_then(|entries| {
            entries.iter().find(|entry| {
                entry["app_slug"] == crate::github::APP_SLUG
                    && entry.pointer("/account/id").and_then(Value::as_u64) == Some(owner)
            })
        })
        .and_then(|entry| entry["id"].as_u64())
        .filter(|id| *id > 0)
        .ok_or(Error::InvalidInstallation)
}

async fn prepare(state: &AppState, create: bool) -> Result<Snapshot> {
    let viewer = commands::session_viewer(state).await?;
    let token = state.access_token().await?;
    let mut cache = cached(state, viewer.id).await?;
    let name = repository_name(viewer.id);
    let endpoint = format!("/repos/{}/{name}", viewer.login);
    let mut response = request(
        &state.github,
        &token,
        Method::GET,
        &endpoint,
        None,
        &[403, 404],
    )
    .await?;
    if response.status != 200 && create {
        response = request(&state.github, &token, Method::POST, "/user/repos", Some(&json!({"name":name,"description":DESCRIPTION,"private":true,"auto_init":true,"has_issues":false,"has_projects":false,"has_wiki":false})), &[403, 404, 422]).await?;
    }
    if !matches!(response.status, 200 | 201) {
        return Ok(cache.snapshot(
            None,
            if response.status == 422 {
                "installation"
            } else {
                "repository"
            },
        ));
    }
    let value = response.data.ok_or(Error::InvalidGitHubResponse)?;
    let repository = normalize_repository(&value, &viewer)?;
    let marker = request(
        &state.github,
        &token,
        Method::GET,
        &format!("{endpoint}/contents/{MARKER}"),
        None,
        &[403, 404],
    )
    .await?;
    if marker.status == 403 {
        return Ok(cache.snapshot(None, "installation"));
    }
    let mut changed = BTreeSet::new();
    if marker.status == 404 {
        if value.get("description").and_then(Value::as_str) != Some(DESCRIPTION) {
            return Err(Error::NoosnoteRepositoryMismatch);
        }
        let value = Arc::new(json!({"version":1,"ownerId":viewer.id}));
        cache.documents.entry(MARKER.into()).or_insert(Document {
            owner_id: viewer.id,
            path: MARKER.into(),
            local: Some(value),
            base: None,
            base_sha: None,
            conflict: None,
        });
        cache.index.dirty.insert(MARKER.into());
        changed.insert(MARKER.into());
    } else {
        let bytes = crate::repository_content::decode_file_bytes(
            marker.data.as_ref().ok_or(Error::InvalidGitHubResponse)?,
            MAX_FILE_BYTES,
            false,
        )?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidData)?;
        validate_marker(&value, viewer.id)?;
    }
    let mut current = state.notes.lock().await;
    if let Some(existing) = current
        .as_ref()
        .filter(|cache| cache.index.owner_id == viewer.id)
    {
        let marker = cache.documents.get(MARKER).cloned();
        cache = existing.clone();
        if let Some(marker) = marker
            && !cache.documents.contains_key(MARKER)
        {
            cache.documents.insert(MARKER.into(), marker);
            cache.index.dirty.insert(MARKER.into());
        }
    }
    cache.index.repository = Some(repository);
    install_cache(state, &mut cache, &changed).await?;
    let result = cache.snapshot(None, "ready");
    *current = Some(cache);
    Ok(result)
}

#[tauri::command]
pub async fn noosnote_cached(state: State<'_, AppState>) -> CommandResult<Snapshot> {
    let viewer = commands::session_viewer(&state)
        .await
        .map_err(|error| error.to_string())?;
    let cache = cached(&state, viewer.id)
        .await
        .map_err(|error| error.to_string())?;
    Ok(cache.snapshot(
        None,
        if cache.index.repository.is_some() {
            "ready"
        } else {
            "repository"
        },
    ))
}

#[tauri::command]
pub async fn noosnote_discover(state: State<'_, AppState>) -> CommandResult<Snapshot> {
    let _guard = state.notes_sync.lock().await;
    prepare(&state, false)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn noosnote_setup(state: State<'_, AppState>) -> CommandResult<Snapshot> {
    let _guard = state.notes_sync.lock().await;
    prepare(&state, true)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn noosnote_open_setup(
    app: DesktopAppHandle,
    state: State<'_, AppState>,
    mode: String,
) -> CommandResult<()> {
    async fn open(app: &DesktopAppHandle, state: &AppState, mode: &str) -> Result<()> {
        let viewer = commands::session_viewer(state).await?;
        let token = state.access_token().await?;
        let url = if mode == "repository" {
            let mut url = url::Url::parse("https://github.com/new").map_err(|_| Error::Local)?;
            url.query_pairs_mut()
                .append_pair("owner", &viewer.login)
                .append_pair("name", &repository_name(viewer.id))
                .append_pair("description", DESCRIPTION)
                .append_pair("visibility", "private");
            url
        } else if mode == "installation" {
            url::Url::parse(&format!(
                "https://github.com/settings/installations/{}",
                installation(state, &token, viewer.id).await?
            ))
            .map_err(|_| Error::Local)?
        } else {
            return Err(Error::InvalidData);
        };
        commands::open_github_url(app, &url).await
    }
    open(&app, &state, &mode)
        .await
        .map_err(|error| error.to_string())
}

fn apply_changes(cache: &mut Cache, changes: Vec<Change>) -> Result<BTreeSet<String>> {
    if changes.is_empty() || changes.len() > 256 {
        return Err(Error::InvalidData);
    }
    let mut changed = BTreeSet::new();
    for change in changes {
        file_kind(&change.path)?;
        if let Some(value) = &change.value {
            validate_file(&change.path, value)?;
        }
        if let Some(value) = change.base.as_ref().and_then(|base| base.value.as_ref()) {
            validate_file(&change.path, value)?;
        }
        let document = cache
            .documents
            .entry(change.path.clone())
            .or_insert_with(|| Document {
                owner_id: cache.index.owner_id,
                path: change.path.clone(),
                local: None,
                base: None,
                base_sha: None,
                conflict: None,
            });
        let mut next = change.value.map(Arc::new);
        if let Some(base) = change.base {
            match merge_value(
                base.value.as_ref(),
                next.as_deref(),
                document.local.as_deref(),
                0,
            ) {
                Ok(merged)
                    if merged
                        .as_ref()
                        .is_none_or(|value| validate_file(&change.path, value).is_ok()) =>
                {
                    next = merged.map(Arc::new);
                }
                _ => {
                    if document.conflict.is_none() {
                        document.conflict = Some(Conflict {
                            id: Uuid::new_v4().to_string(),
                            path: change.path.clone(),
                            local: next.clone(),
                            remote: document.local.clone(),
                        });
                    }
                }
            }
        }
        if document.local == next {
            continue;
        }
        document.local = next;
        if let Some(conflict) = &mut document.conflict {
            conflict.local = document.local.clone();
        }
        if document.local == document.base && document.conflict.is_none() {
            cache.index.dirty.remove(&change.path);
        } else {
            cache.index.dirty.insert(change.path.clone());
        }
        changed.insert(change.path);
    }
    if cache.index.blobs.len()
        + changed
            .iter()
            .filter(|path| !cache.index.blobs.contains_key(*path))
            .count()
        > MAX_FILES
    {
        return Err(Error::ResponseTooLarge);
    }
    Ok(changed)
}

#[tauri::command]
pub async fn noosnote_save(
    state: State<'_, AppState>,
    changes: Vec<Change>,
) -> CommandResult<Snapshot> {
    async fn save(state: &AppState, changes: Vec<Change>) -> Result<Snapshot> {
        let viewer = commands::session_viewer(state).await?;
        cached(state, viewer.id).await?;
        let mut current = state.notes.lock().await;
        let mut cache = current
            .as_ref()
            .filter(|cache| cache.index.owner_id == viewer.id)
            .cloned()
            .ok_or(Error::SessionExpired)?;
        let changed = apply_changes(&mut cache, changes)?;
        install_cache(state, &mut cache, &changed).await?;
        let snapshot = cache.snapshot(
            Some(&changed),
            if cache.index.repository.is_some() {
                "ready"
            } else {
                "repository"
            },
        );
        *current = Some(cache);
        Ok(snapshot)
    }
    save(&state, changes)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn noosnote_resolve(
    state: State<'_, AppState>,
    id: String,
    choice: String,
) -> CommandResult<Snapshot> {
    async fn resolve(state: &AppState, id: &str, choice: &str) -> Result<Snapshot> {
        if !uuid(id) || !matches!(choice, "local" | "remote") {
            return Err(Error::InvalidData);
        }
        let viewer = commands::session_viewer(state).await?;
        cached(state, viewer.id).await?;
        let mut current = state.notes.lock().await;
        let mut cache = current
            .as_ref()
            .filter(|cache| cache.index.owner_id == viewer.id)
            .cloned()
            .ok_or(Error::SessionExpired)?;
        let (path, document) = cache
            .documents
            .iter_mut()
            .find(|(_, document)| {
                document
                    .conflict
                    .as_ref()
                    .is_some_and(|conflict| conflict.id == id)
            })
            .ok_or(Error::InvalidData)?;
        let conflict = document.conflict.take().ok_or(Error::InvalidData)?;
        let recovery = serde_json::to_string(&conflict).map_err(|_| Error::Local)?;
        state
            .blobs
            .save(&format!("nn-recovery-{id}"), SecretString::from(recovery))?;
        document.local = if choice == "local" {
            conflict.local
        } else {
            conflict.remote
        };
        if document.local == document.base {
            cache.index.dirty.remove(path);
        } else {
            cache.index.dirty.insert(path.clone());
        }
        let changed = BTreeSet::from([path.clone()]);
        install_cache(state, &mut cache, &changed).await?;
        let snapshot = cache.snapshot(Some(&changed), "ready");
        *current = Some(cache);
        Ok(snapshot)
    }
    resolve(&state, &id, &choice)
        .await
        .map_err(|error| error.to_string())
}

fn commit_tree(cache: &Cache) -> Result<Vec<Value>> {
    let mut tree = Vec::new();
    for path in &cache.index.dirty {
        let document = cache.documents.get(path).ok_or(Error::Local)?;
        if document.conflict.is_some() {
            return Err(Error::NoosnoteConflict);
        }
        let mut entry = json!({"path":path,"mode":"100644","type":"blob"});
        match &document.local {
            Some(value) => {
                entry["content"] =
                    Value::String(serde_json::to_string(value).map_err(|_| Error::Local)? + "\n");
            }
            None if document.base.is_some() => {
                entry["sha"] = Value::Null;
            }
            None => continue,
        }
        tree.push(entry);
    }
    Ok(tree)
}

async fn read_documents(
    state: &AppState,
    token: &SecretString,
    viewer: &GitHubViewer,
    repository: &Repository,
    hashes: &BTreeMap<String, String>,
    previous: &BTreeMap<String, String>,
    sizes: &BTreeMap<String, u64>,
) -> Result<BTreeMap<String, Option<Arc<Value>>>> {
    let pending: Vec<_> = hashes
        .iter()
        .filter(|(path, hash)| previous.get(*path) != Some(*hash))
        .collect();
    let mut values = BTreeMap::new();
    let mut position = 0;
    while position < pending.len() {
        let start = position;
        let mut bytes = 0;
        while position < pending.len() && position - start < 16 {
            let size = sizes
                .get(pending[position].0)
                .copied()
                .unwrap_or(MAX_FILE_BYTES as u64);
            if position > start && bytes + size > 2 * 1024 * 1024 {
                break;
            }
            bytes += size;
            position += 1;
        }
        let batch = &pending[start..position];
        let objects = batch.iter().enumerate().map(|(index, (_, hash))| format!("f{index}:object(oid:\"{hash}\") {{ ... on Blob {{ oid byteSize text isTruncated }} }}")).collect::<Vec<_>>().join(" ");
        let query = format!(
            "query Notes($owner:String!,$name:String!) {{ repository(owner:$owner,name:$name) {{ isPrivate databaseId {objects} }} }}"
        );
        let response = request(
            &state.github,
            token,
            Method::POST,
            "/graphql",
            Some(&json!({"query":query,"variables":{"owner":viewer.login,"name":repository.name}})),
            &[],
        )
        .await?;
        let response = response.data.ok_or(Error::InvalidGitHubResponse)?;
        if let Some(errors) = response.get("errors").and_then(Value::as_array) {
            if errors
                .iter()
                .any(|error| error.get("type").and_then(Value::as_str) == Some("RATE_LIMITED"))
            {
                return Err(Error::RateLimited);
            }
            if !errors.is_empty() {
                return Err(Error::GitHubDenied);
            }
        }
        let remote = response
            .pointer("/data/repository")
            .ok_or(Error::InvalidGitHubResponse)?;
        if remote.get("isPrivate").and_then(Value::as_bool) != Some(true) {
            return Err(Error::NoosnoteRepositoryPublic);
        }
        if remote.get("databaseId").and_then(Value::as_u64) != Some(repository.id) {
            return Err(Error::NoosnoteRepositoryMismatch);
        }
        for (index, (path, hash)) in batch.iter().enumerate() {
            let blob = remote
                .get(format!("f{index}"))
                .ok_or(Error::InvalidGitHubResponse)?;
            if sha(blob, "oid")? != **hash {
                return Err(Error::InvalidGitHubResponse);
            }
            let value = if blob.get("isTruncated").and_then(Value::as_bool) == Some(true) {
                let response = request(
                    &state.github,
                    token,
                    Method::GET,
                    &format!(
                        "/repos/{}/{}/git/blobs/{hash}",
                        viewer.login, repository.name
                    ),
                    None,
                    &[],
                )
                .await?;
                let mut content = response.data.ok_or(Error::InvalidGitHubResponse)?;
                if sha(&content, "sha")? != **hash {
                    return Err(Error::InvalidGitHubResponse);
                }
                content["type"] = Value::String("file".into());
                serde_json::from_slice(&crate::repository_content::decode_file_bytes(
                    &content,
                    MAX_FILE_BYTES,
                    false,
                )?)
                .map_err(|_| Error::InvalidData)?
            } else {
                let content = field(blob, "text", MAX_FILE_BYTES)?;
                if blob.get("byteSize").and_then(Value::as_u64) != Some(content.len() as u64) {
                    return Err(Error::InvalidGitHubResponse);
                }
                serde_json::from_str(content).map_err(|_| Error::InvalidData)?
            };
            values.insert((*path).clone(), Some(Arc::new(value)));
        }
    }
    Ok(values)
}

fn record_publish(latest: &mut Cache, sent: &Cache) -> Result<BTreeSet<String>> {
    let mut persisted = BTreeSet::new();
    for path in &sent.index.dirty {
        let sent = sent.documents.get(path).ok_or(Error::Local)?;
        if let Some(document) = latest.documents.get_mut(path) {
            document.base = sent.local.clone();
            document.base_sha = None;
            if document.local == document.base && document.conflict.is_none() {
                latest.index.dirty.remove(path);
            }
            latest.index.hashes.remove(path);
            persisted.insert(path.clone());
        }
    }
    latest.index.head = None;
    latest.index.last_synced_at = Some(now());
    Ok(persisted)
}

async fn sync(state: &AppState) -> Result<Snapshot> {
    let viewer = commands::session_viewer(state).await?;
    let token = state.access_token().await?;
    let initial = cached(state, viewer.id).await?;
    if initial.index.repository.is_none() {
        return prepare(state, false).await;
    }
    let root = format!("/repos/{}/{}", viewer.login, repository_name(viewer.id));
    let repository_response =
        request(&state.github, &token, Method::GET, &root, None, &[403, 404]).await?;
    if repository_response.status != 200 {
        return Ok(initial.snapshot(Some(&BTreeSet::new()), "installation"));
    }
    let repository = normalize_repository(
        repository_response
            .data
            .as_ref()
            .ok_or(Error::InvalidGitHubResponse)?,
        &viewer,
    )?;
    if initial
        .index
        .repository
        .as_ref()
        .is_some_and(|previous| previous.id != repository.id)
    {
        return Err(Error::NoosnoteRepositoryMismatch);
    }
    let reference = format!("{root}/git/ref/heads/{}", segment(&repository.branch));
    let response = request(
        &state.github,
        &token,
        Method::GET,
        &reference,
        None,
        &[404, 409],
    )
    .await?;
    let head = if matches!(response.status, 404 | 409)
        && initial
            .documents
            .values()
            .all(|document| document.base.is_none())
    {
        let marker = json!({"version":1,"ownerId":viewer.id});
        let response = request(&state.github, &token, Method::PUT, &format!("{root}/contents/{MARKER}"), Some(&json!({"message":"Initialize notes","content":STANDARD.encode(serde_json::to_vec(&marker).map_err(|_| Error::Local)?),"branch":repository.branch})), &[]).await?;
        sha(
            response
                .data
                .as_ref()
                .and_then(|value| value.get("commit"))
                .ok_or(Error::InvalidGitHubResponse)?,
            "sha",
        )?
    } else {
        sha(
            response
                .data
                .as_ref()
                .and_then(|value| value.get("object"))
                .ok_or(Error::InvalidGitHubResponse)?,
            "sha",
        )?
    };
    if initial.index.head.as_ref() == Some(&head)
        && initial.index.dirty.is_empty()
        && initial
            .documents
            .iter()
            .all(|(path, document)| document.base_sha.as_ref() == initial.index.hashes.get(path))
    {
        return Ok(initial.snapshot(Some(&BTreeSet::new()), "ready"));
    }
    let response = request(
        &state.github,
        &token,
        Method::GET,
        &format!("{root}/git/trees/{head}?recursive=1"),
        None,
        &[],
    )
    .await?;
    let tree = response.data.ok_or(Error::InvalidGitHubResponse)?;
    if tree.get("truncated").and_then(Value::as_bool) != Some(false) {
        return Err(Error::ResponseTooLarge);
    }
    let tree_sha = sha(&tree, "sha")?;
    let mut hashes = BTreeMap::new();
    let mut sizes = BTreeMap::new();
    for entry in array(&tree, "tree", 30_000)? {
        let path = field(entry, "path", 512)?;
        if (path == MARKER || file_kind(path).is_ok()) && entry["type"] == "blob" {
            if entry["mode"] != "100644"
                || entry
                    .get("size")
                    .and_then(Value::as_u64)
                    .is_none_or(|size| size > MAX_FILE_BYTES as u64)
            {
                return Err(Error::InvalidData);
            }
            hashes.insert(path.to_owned(), sha(entry, "sha")?);
            sizes.insert(
                path.to_owned(),
                entry["size"].as_u64().ok_or(Error::InvalidGitHubResponse)?,
            );
        }
    }
    if hashes.len() > MAX_FILES {
        return Err(Error::ResponseTooLarge);
    }
    let requested = hashes
        .iter()
        .filter(|(path, _)| {
            !file_kind(path).is_ok_and(|(kind, _, _)| kind == "page")
                || initial.documents.contains_key(*path)
        })
        .map(|(path, hash)| (path.clone(), hash.clone()))
        .collect();
    let previous = initial
        .documents
        .iter()
        .filter_map(|(path, document)| {
            document
                .base_sha
                .as_ref()
                .map(|hash| (path.clone(), hash.clone()))
        })
        .collect();
    let mut values = read_documents(
        state,
        &token,
        &viewer,
        &repository,
        &requested,
        &previous,
        &sizes,
    )
    .await?;
    for path in initial
        .documents
        .keys()
        .filter(|path| !hashes.contains_key(*path))
    {
        values.insert(path.clone(), None);
    }
    if !hashes.contains_key(MARKER) && !initial.index.dirty.contains(MARKER) {
        return Err(Error::NoosnoteRepositoryMismatch);
    }
    let mut current = state.notes.lock().await;
    let mut cache = current
        .as_ref()
        .filter(|cache| cache.index.owner_id == viewer.id)
        .cloned()
        .ok_or(Error::SessionExpired)?;
    let changed = merge_remote(&mut cache, values, hashes, head.clone())?;
    cache.index.repository = Some(repository.clone());
    install_cache(state, &mut cache, &changed).await?;
    *current = Some(cache.clone());
    drop(current);
    if cache
        .documents
        .values()
        .any(|document| document.conflict.is_some())
        || cache.index.dirty.is_empty()
    {
        return Ok(cache.snapshot(Some(&changed), "ready"));
    }
    let updates = commit_tree(&cache)?;
    if updates.is_empty() {
        return Ok(cache.snapshot(Some(&changed), "ready"));
    }
    if commands::session_viewer(state).await?.id != viewer.id {
        return Err(Error::SessionExpired);
    }
    let response = request(
        &state.github,
        &token,
        Method::POST,
        &format!("{root}/git/trees"),
        Some(&json!({"base_tree":tree_sha,"tree":updates})),
        &[],
    )
    .await?;
    let next_tree = sha(
        response.data.as_ref().ok_or(Error::InvalidGitHubResponse)?,
        "sha",
    )?;
    let response = request(
        &state.github,
        &token,
        Method::POST,
        &format!("{root}/git/commits"),
        Some(&json!({"message":"Update notes","tree":next_tree,"parents":[head]})),
        &[],
    )
    .await?;
    let commit = sha(
        response.data.as_ref().ok_or(Error::InvalidGitHubResponse)?,
        "sha",
    )?;
    let response = request(
        &state.github,
        &token,
        Method::PATCH,
        &format!("{root}/git/refs/heads/{}", segment(&repository.branch)),
        Some(&json!({"sha":commit,"force":false})),
        &[409, 422],
    )
    .await?;
    if matches!(response.status, 409 | 422) {
        crate::startup::write_log(format_args!(
            "[noosnote] phase=publish concurrentUpdate=true"
        ));
        return Err(Error::NoosnoteConcurrentUpdate);
    }
    let mut current = state.notes.lock().await;
    let mut latest = current
        .as_ref()
        .filter(|cache| cache.index.owner_id == viewer.id)
        .cloned()
        .ok_or(Error::SessionExpired)?;
    let persisted = record_publish(&mut latest, &cache)?;
    install_cache(state, &mut latest, &persisted).await?;
    let result = latest.snapshot(Some(&changed), "ready");
    evict_pages(&mut latest);
    *current = Some(latest);
    Ok(result)
}

#[tauri::command]
pub async fn noosnote_sync(state: State<'_, AppState>) -> CommandResult<Snapshot> {
    let _guard = state.notes_sync.lock().await;
    let result = sync(&state).await;
    if let Err(error) = &result {
        crate::startup::write_log(format_args!("[noosnote] phase=sync error={error:?}"));
    }
    result.map_err(|error| error.to_string())
}

async fn load_page(state: &AppState, workspace_id: &str, page_id: &str) -> Result<Snapshot> {
    if !uuid(workspace_id) || !uuid(page_id) {
        return Err(Error::InvalidData);
    }
    let viewer = commands::session_viewer(state).await?;
    let path = format!("notes/{workspace_id}/pages/{page_id}.json");
    let initial = cached(state, viewer.id).await?;
    let mut fetched = false;
    let document = if let Some(document) = initial.documents.get(&path) {
        document.clone()
    } else if let Some(key) = initial.index.blobs.get(&path) {
        let store = state.blobs.clone();
        let path = path.clone();
        let key = key.clone();
        tokio::task::spawn_blocking(move || load_document(&store, viewer.id, &path, &key))
            .await
            .map_err(|_| Error::Local)??
    } else {
        let repository = initial
            .index
            .repository
            .as_ref()
            .ok_or(Error::NoosnotePageUnavailable)?;
        let hash = initial
            .index
            .hashes
            .get(&path)
            .ok_or(Error::NoosnotePageUnavailable)?;
        let token = state.access_token().await?;
        let hashes = BTreeMap::from([(path.clone(), hash.clone())]);
        let values = read_documents(
            state,
            &token,
            &viewer,
            repository,
            &hashes,
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .await?;
        let value = values
            .get(&path)
            .cloned()
            .flatten()
            .ok_or(Error::NoosnotePageUnavailable)?;
        validate_file(&path, &value)?;
        fetched = true;
        Document {
            owner_id: viewer.id,
            path: path.clone(),
            local: Some(value.clone()),
            base: Some(value),
            base_sha: Some(hash.clone()),
            conflict: None,
        }
    };
    let mut current = state.notes.lock().await;
    let mut cache = current
        .as_ref()
        .filter(|cache| cache.index.owner_id == viewer.id)
        .cloned()
        .ok_or(Error::SessionExpired)?;
    cache.documents.entry(path.clone()).or_insert(document);
    touch_page(&mut cache, &path);
    let changed = BTreeSet::from([path.clone()]);
    if fetched {
        install_cache(state, &mut cache, &changed).await?;
    }
    evict_pages(&mut cache);
    let snapshot = cache.snapshot(Some(&changed), "ready");
    *current = Some(cache);
    Ok(snapshot)
}

#[tauri::command]
pub async fn noosnote_page(
    state: State<'_, AppState>,
    workspace_id: String,
    page_id: String,
) -> CommandResult<Snapshot> {
    load_page(&state, &workspace_id, &page_id)
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> (String, Value) {
        let workspace = Uuid::new_v4().to_string();
        let id = Uuid::new_v4().to_string();
        (
            format!("notes/{workspace}/pages/{id}.json"),
            json!({"version":1,"kind":"page","id":id,"workspaceId":workspace,"title":"Anglais","icon":"","content":{"type":"doc","content":[{"type":"paragraph","attrs":{"id":Uuid::new_v4().to_string()},"content":[{"type":"text","text":"Cours du lundi"}]}]}}),
        )
    }

    #[test]
    fn account_repository_must_be_private_and_owned_by_the_same_user() {
        let viewer = GitHubViewer {
            id: 42,
            login: "octocat".into(),
            avatar_url: String::new(),
            name: None,
            repository: crate::models::RepositorySummary {
                id: 420,
                name: "noosphere_user_octocat".into(),
                url: String::new(),
                created: false,
            },
        };
        let repository = json!({"id":421,"name":"noosnote_42","private":true,"owner":{"id":42},"default_branch":"main"});
        assert_eq!(normalize_repository(&repository, &viewer).unwrap().id, 421);
        let mut public = repository.clone();
        public["private"] = json!(false);
        assert!(matches!(
            normalize_repository(&public, &viewer),
            Err(Error::NoosnoteRepositoryPublic)
        ));
        let mut foreign = repository;
        foreign["owner"]["id"] = json!(99);
        assert!(matches!(
            normalize_repository(&foreign, &viewer),
            Err(Error::NoosnoteRepositoryMismatch)
        ));
        assert!(validate_marker(&json!({"version":1,"ownerId":99}), 42).is_err());
    }

    #[test]
    fn paths_cannot_escape_the_note_directory() {
        let (path, value) = page();
        assert!(validate_file(&path, &value).is_ok());
        for path in [
            ".noosphere.json",
            "notes/../../profile.json",
            "notes/not-an-id/pages/a.json",
            "notes/42/pages/a.json",
            "notes/42/workspace.json",
        ] {
            assert!(file_kind(path).is_err());
        }
        let mut value = value;
        value["workspaceId"] = json!(Uuid::new_v4().to_string());
        assert!(validate_file(&path, &value).is_err());
    }

    #[test]
    fn editor_school_documents_and_views_match_the_native_schema() {
        let files: BTreeMap<String, Value> =
            serde_json::from_str(include_str!("../../tests/fixtures/noosnote-school.json"))
                .unwrap();
        for (path, value) in &files {
            validate_file(path, value).unwrap();
        }
        let (path, value) = files
            .iter()
            .find(|(_, value)| value["kind"] == "database")
            .unwrap();
        let mut database = value.clone();
        let column = database["columns"][0]["id"].clone();
        database["views"][0]["filters"] = json!([{"id":Uuid::new_v4().to_string(),"propertyId":column,"operator":"contains","value":"Anglais"}]);
        database["views"][0]["sorts"] = json!([{"propertyId":column,"direction":"asc"}]);
        validate_file(path, &database).unwrap();
        database["views"][0]["filters"][0]["operator"] = json!("execute");
        assert!(validate_file(path, &database).is_err());
        database["views"][0]["filters"] = json!([]);
        database["views"][0]["sorts"][0]["propertyId"] = json!(Uuid::new_v4().to_string());
        assert!(validate_file(path, &database).is_err());
    }

    #[test]
    fn rich_text_rejects_executable_links_and_unknown_nodes() {
        let (path, mut value) = page();
        value["content"]["content"][0]["content"][0]["marks"] =
            json!([{"type":"link","attrs":{"href":"javascript:alert(1)"}}]);
        assert!(validate_file(&path, &value).is_err());
        value["content"]["content"][0] = json!({"type":"script","text":"alert(1)"});
        assert!(validate_file(&path, &value).is_err());
    }

    #[test]
    fn independent_fields_and_text_edits_merge_without_losing_content() {
        let base = json!({"title":"Cours","text":"Bonjour le monde"});
        let local = json!({"title":"Anglais","text":"Bonjour le monde"});
        let remote = json!({"title":"Cours","text":"Bonjour toute la classe"});
        assert_eq!(
            merge_value(Some(&base), Some(&local), Some(&remote), 0).unwrap(),
            Some(json!({"title":"Anglais","text":"Bonjour toute la classe"}))
        );
        assert_eq!(
            merge_text(
                "English and French",
                "Advanced English and French",
                "English and Spanish"
            ),
            Some("Advanced English and Spanish".into())
        );
        assert!(merge_text("French", "Spanish", "German").is_none());
    }

    #[test]
    fn separate_blocks_and_new_database_rows_are_preserved() {
        let base = json!([{"id":"a","values":{"category":"Cours","score":10}}]);
        let local =
            json!([{"id":"a","values":{"category":"Langues","score":10}},{"id":"b","values":{}}]);
        let remote =
            json!([{"id":"a","values":{"category":"Cours","score":15}},{"id":"c","values":{}}]);
        let merged = merge_value(Some(&base), Some(&local), Some(&remote), 0)
            .unwrap()
            .unwrap();
        assert_eq!(
            merged[0],
            json!({"id":"a","values":{"category":"Langues","score":15}})
        );
        assert_eq!(merged.as_array().unwrap().len(), 3);
    }

    #[test]
    fn deleted_pages_and_concurrent_edits_require_a_choice() {
        let (path, base) = page();
        let mut cache = Cache::empty(42);
        apply_changes(
            &mut cache,
            vec![Change {
                base: None,
                path: path.clone(),
                value: Some(base.clone()),
            }],
        )
        .unwrap();
        merge_remote(
            &mut cache,
            BTreeMap::from([(path.clone(), Some(Arc::new(base)))]),
            BTreeMap::from([(path.clone(), "a".repeat(40))]),
            "b".repeat(40),
        )
        .unwrap();
        let mut edited = cache.documents[&path].local.as_deref().unwrap().clone();
        edited["content"]["content"][0]["content"][0]["text"] = json!("Nouveau cours");
        apply_changes(
            &mut cache,
            vec![Change {
                base: None,
                path: path.clone(),
                value: Some(edited.clone()),
            }],
        )
        .unwrap();
        merge_remote(
            &mut cache,
            BTreeMap::from([(path.clone(), None)]),
            BTreeMap::new(),
            "c".repeat(40),
        )
        .unwrap();
        let conflict = cache.documents[&path].conflict.as_ref().unwrap();
        assert_eq!(conflict.local.as_deref(), Some(&edited));
        assert!(conflict.remote.is_none());
        assert!(commit_tree(&cache).is_err());
    }

    #[test]
    fn protected_drafts_and_conflicts_survive_a_restart() {
        let temporary = tempfile::tempdir().unwrap();
        let store = SecureBlobStore::new(temporary.path()).unwrap();
        let (path, value) = page();
        let mut cache = Cache::empty(42);
        let changed = apply_changes(
            &mut cache,
            vec![Change {
                base: None,
                path: path.clone(),
                value: Some(value.clone()),
            }],
        )
        .unwrap();
        persist_cache(&store, &mut cache, &changed).unwrap();
        let restored = load_cache(&store, 42).unwrap();
        assert_eq!(restored.documents[&path].local.as_deref(), Some(&value));
        assert!(restored.index.dirty.contains(&path));
        let mut remote = value.clone();
        remote["title"] = json!("Remote title");
        let mut base = value.clone();
        base["title"] = json!("Original title");
        cache.documents.get_mut(&path).unwrap().base = Some(Arc::new(base));
        let changed = merge_remote(
            &mut cache,
            BTreeMap::from([(path.clone(), Some(Arc::new(remote.clone())))]),
            BTreeMap::from([(path.clone(), "a".repeat(40))]),
            "b".repeat(40),
        )
        .unwrap();
        persist_cache(&store, &mut cache, &changed).unwrap();
        let conflicted = load_cache(&store, 42).unwrap();
        assert_eq!(
            conflicted.documents[&path]
                .conflict
                .as_ref()
                .unwrap()
                .remote
                .as_deref(),
            Some(&remote)
        );
        assert_eq!(
            conflicted.documents[&path]
                .conflict
                .as_ref()
                .unwrap()
                .local
                .as_deref(),
            Some(&value)
        );
        assert!(load_cache(&store, 99).unwrap().documents.is_empty());
        let mut foreign = cache.index.clone();
        foreign.owner_id = 99;
        store
            .save(
                &index_account(99),
                SecretString::from(serde_json::to_string(&foreign).unwrap()),
            )
            .unwrap();
        assert!(load_cache(&store, 99).is_err());
    }

    #[test]
    fn protected_document_keys_fit_the_store_for_large_account_ids() {
        let temporary = tempfile::tempdir().unwrap();
        let store = SecureBlobStore::new(temporary.path()).unwrap();
        let (path, value) = page();
        let mut cache = Cache::empty(u64::MAX);
        let changed = apply_changes(
            &mut cache,
            vec![Change {
                path: path.clone(),
                value: Some(value.clone()),
                base: None,
            }],
        )
        .unwrap();
        persist_cache(&store, &mut cache, &changed).unwrap();
        assert!(cache.index.blobs[&path].len() <= 64);
        assert_ne!(document_prefix(42, &path), document_prefix(99, &path));
        let restored = load_cache(&store, u64::MAX).unwrap();
        assert_eq!(restored.documents[&path].local.as_deref(), Some(&value));
    }

    #[test]
    fn repeated_checkpoints_keep_one_protected_copy_per_document() {
        let temporary = tempfile::tempdir().unwrap();
        let store = SecureBlobStore::new(temporary.path()).unwrap();
        let (path, mut value) = page();
        let mut cache = Cache::empty(42);
        for revision in 0..10 {
            value["title"] = json!(format!("Cours {revision}"));
            let changed = apply_changes(
                &mut cache,
                vec![Change {
                    path: path.clone(),
                    value: Some(value.clone()),
                    base: None,
                }],
            )
            .unwrap();
            persist_cache(&store, &mut cache, &changed).unwrap();
        }
        let prefix = document_prefix(42, &path);
        let copies = std::fs::read_dir(temporary.path().join("protected"))
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
            .count();
        assert_eq!(copies, 1);
        assert_eq!(
            load_cache(&store, 42).unwrap().documents[&path]
                .local
                .as_deref(),
            Some(&value)
        );
    }

    #[test]
    fn clean_pages_are_loaded_only_when_opened_and_can_be_evicted() {
        let temporary = tempfile::tempdir().unwrap();
        let store = SecureBlobStore::new(temporary.path()).unwrap();
        let (path, value) = page();
        let mut cache = Cache::empty(42);
        let changed = merge_remote(
            &mut cache,
            BTreeMap::from([(path.clone(), Some(Arc::new(value.clone())))]),
            BTreeMap::from([(path.clone(), "a".repeat(40))]),
            "b".repeat(40),
        )
        .unwrap();
        persist_cache(&store, &mut cache, &changed).unwrap();
        let mut restored = load_cache(&store, 42).unwrap();
        assert!(!restored.documents.contains_key(&path));
        let document = load_document(&store, 42, &path, &restored.index.blobs[&path]).unwrap();
        assert_eq!(document.local.as_deref(), Some(&value));
        restored.documents.insert(path.clone(), document);
        touch_page(&mut restored, &path);
        evict_pages(&mut restored);
        assert!(restored.documents.contains_key(&path));
        restored.opened.clear();
        evict_pages(&mut restored);
        assert!(!restored.documents.contains_key(&path));
        assert!(restored.index.blobs.contains_key(&path));
    }

    #[test]
    fn edits_made_while_a_commit_is_in_flight_remain_pending() {
        let (path, value) = page();
        let mut sent = Cache::empty(42);
        apply_changes(
            &mut sent,
            vec![Change {
                base: None,
                path: path.clone(),
                value: Some(value.clone()),
            }],
        )
        .unwrap();
        let mut latest = sent.clone();
        let mut edited = value.clone();
        edited["title"] = json!("Anglais — mardi");
        apply_changes(
            &mut latest,
            vec![Change {
                base: None,
                path: path.clone(),
                value: Some(edited.clone()),
            }],
        )
        .unwrap();
        record_publish(&mut latest, &sent).unwrap();
        assert_eq!(latest.documents[&path].base.as_deref(), Some(&value));
        assert_eq!(latest.documents[&path].local.as_deref(), Some(&edited));
        assert!(latest.index.dirty.contains(&path));
    }

    #[test]
    fn workspace_parent_cycles_are_rejected() {
        let workspace = Uuid::new_v4().to_string();
        let a = Uuid::new_v4().to_string();
        let b = Uuid::new_v4().to_string();
        let value = json!({"version":1,"kind":"workspace","id":workspace,"title":"École","icon":"📚","color":"#78b89b","pages":[{"id":a,"title":"Anglais","icon":"","parentId":b},{"id":b,"title":"Français","icon":"","parentId":a}]});
        assert!(validate_file(&format!("notes/{workspace}/workspace.json"), &value).is_err());
    }

    #[test]
    fn typing_during_a_pull_keeps_the_received_changes() {
        let (path, base) = page();
        let mut cache = Cache::empty(42);
        let mut remote = base.clone();
        remote["title"] = json!("Advanced English");
        merge_remote(
            &mut cache,
            BTreeMap::from([(path.clone(), Some(Arc::new(remote)))]),
            BTreeMap::from([(path.clone(), "a".repeat(40))]),
            "b".repeat(40),
        )
        .unwrap();
        let mut typed = base.clone();
        typed["content"]["content"][0]["content"][0]["text"] = json!("Cours du lundi et mardi");
        apply_changes(
            &mut cache,
            vec![Change {
                path: path.clone(),
                value: Some(typed),
                base: Some(EditBase { value: Some(base) }),
            }],
        )
        .unwrap();
        let document = &cache.documents[&path];
        assert!(document.conflict.is_none());
        let value = document.local.as_deref().unwrap();
        assert_eq!(value["title"], "Advanced English");
        assert_eq!(
            value["content"]["content"][0]["content"][0]["text"],
            "Cours du lundi et mardi"
        );
    }

    #[test]
    fn overlapping_typing_and_received_changes_keep_both_versions() {
        let (path, base) = page();
        let mut cache = Cache::empty(42);
        let mut remote = base.clone();
        remote["content"]["content"][0]["content"][0]["text"] = json!("Cours du vendredi");
        merge_remote(
            &mut cache,
            BTreeMap::from([(path.clone(), Some(Arc::new(remote.clone())))]),
            BTreeMap::from([(path.clone(), "a".repeat(40))]),
            "b".repeat(40),
        )
        .unwrap();
        let mut typed = base.clone();
        typed["content"]["content"][0]["content"][0]["text"] = json!("Cours du mercredi");
        apply_changes(
            &mut cache,
            vec![Change {
                path: path.clone(),
                value: Some(typed.clone()),
                base: Some(EditBase { value: Some(base) }),
            }],
        )
        .unwrap();
        let conflict = cache.documents[&path].conflict.as_ref().unwrap();
        assert_eq!(conflict.local.as_deref(), Some(&typed));
        assert_eq!(conflict.remote.as_deref(), Some(&remote));
        assert!(commit_tree(&cache).is_err());
    }

    #[test]
    fn deep_page_trees_are_validated_without_repeated_parent_scans() {
        let workspace = Uuid::new_v4().to_string();
        let mut parent = None;
        let pages: Vec<_> = (0..4096)
            .map(|_| {
                let id = Uuid::new_v4().to_string();
                let page = json!({"id":id,"title":"Cours","icon":"","parentId":parent});
                parent = Some(id);
                page
            })
            .collect();
        let value = json!({"version":1,"kind":"workspace","id":workspace,"title":"École","icon":"📚","color":"#78b89b","pages":pages});
        assert!(validate_file(&format!("notes/{workspace}/workspace.json"), &value).is_ok());
    }

    type HttpRequest = (String, Value);
    type HttpResponses = Vec<(&'static str, u16, Value)>;

    fn http_fixture(
        responses: HttpResponses,
    ) -> (url::Url, std::thread::JoinHandle<Vec<HttpRequest>>) {
        use std::io::{Read as _, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let origin =
            url::Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let worker = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for (expected, status, response) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                while !bytes.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    bytes.push(byte[0]);
                    assert!(bytes.len() < 16 * 1024);
                }
                let headers = String::from_utf8(bytes).unwrap();
                let first = headers
                    .lines()
                    .next()
                    .unwrap()
                    .strip_suffix(" HTTP/1.1")
                    .unwrap()
                    .to_owned();
                assert_eq!(first, expected);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|value| value.parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                assert!(length <= 4 * 1024 * 1024);
                let mut body = vec![0; length];
                stream.read_exact(&mut body).unwrap();
                requests.push((
                    first,
                    if body.is_empty() {
                        Value::Null
                    } else {
                        serde_json::from_slice(&body).unwrap()
                    },
                ));
                let body = serde_json::to_string(&response).unwrap();
                write!(stream,"HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            }
            requests
        });
        (origin, worker)
    }

    fn viewer() -> GitHubViewer {
        GitHubViewer {
            id: 42,
            login: "octocat".into(),
            avatar_url: String::new(),
            name: None,
            repository: crate::models::RepositorySummary {
                id: 420,
                name: "noosphere_user_octocat".into(),
                url: "https://github.com/octocat/noosphere_user_octocat".into(),
                created: false,
            },
        }
    }

    async fn state(directory: &std::path::Path, origin: url::Url) -> AppState {
        let mut state =
            AppState::new(crate::instance_profile::InstanceProfile::acquire(directory).unwrap())
                .unwrap();
        state.github.test_origin = Some(origin);
        *state.session.write().await = Some(crate::state::Session {
            access_token: SecretString::from("A".repeat(64)),
            refresh_token: None,
            token_expires_at: None,
            refresh_expires_at: None,
            viewer: viewer(),
        });
        state
    }

    fn repository() -> Value {
        json!({"id":421,"name":"noosnote_42","private":true,"owner":{"id":42,"login":"octocat"},"default_branch":"main","description":DESCRIPTION})
    }

    fn workspace(page: &Value) -> (String, Value) {
        let id = page["workspaceId"].as_str().unwrap();
        (
            format!("notes/{id}/workspace.json"),
            json!({"version":1,"kind":"workspace","id":id,"title":"École","icon":"📚","color":"#78b89b","pages":[{"id":page["id"],"title":page["title"],"icon":"","parentId":null}]}),
        )
    }

    async fn publish_fixture(status: u16) -> (Snapshot, Cache, Vec<HttpRequest>) {
        let (origin, server) = http_fixture(vec![
            (
                "GET /repos/octocat/noosnote_42",
                404,
                json!({"message":"Not Found"}),
            ),
            ("POST /user/repos", 201, repository()),
            (
                "GET /repos/octocat/noosnote_42/contents/.noosnote.json",
                404,
                json!({"message":"Not Found"}),
            ),
            ("GET /repos/octocat/noosnote_42", 200, repository()),
            (
                "GET /repos/octocat/noosnote_42/git/ref/heads/main",
                200,
                json!({"object":{"sha":"a".repeat(40)}}),
            ),
            (
                "GET /repos/octocat/noosnote_42/git/trees/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa?recursive=1",
                200,
                json!({"sha":"b".repeat(40),"truncated":false,"tree":[]}),
            ),
            (
                "POST /repos/octocat/noosnote_42/git/trees",
                201,
                json!({"sha":"c".repeat(40)}),
            ),
            (
                "POST /repos/octocat/noosnote_42/git/commits",
                201,
                json!({"sha":"d".repeat(40)}),
            ),
            (
                "PATCH /repos/octocat/noosnote_42/git/refs/heads/main",
                status,
                json!({"message":"fixture"}),
            ),
        ]);
        let temporary = tempfile::tempdir().unwrap();
        let state = state(temporary.path(), origin).await;
        assert_eq!(prepare(&state, true).await.unwrap().setup, "ready");
        let (page_path, page) = page();
        let (workspace_path, workspace) = workspace(&page);
        let mut cache = cached(&state, 42).await.unwrap();
        let changed = apply_changes(
            &mut cache,
            vec![
                Change {
                    base: None,
                    path: page_path.clone(),
                    value: Some(page),
                },
                Change {
                    base: None,
                    path: workspace_path,
                    value: Some(workspace),
                },
            ],
        )
        .unwrap();
        install_cache(&state, &mut cache, &changed).await.unwrap();
        *state.notes.lock().await = Some(cache);
        let result = sync(&state).await;
        let snapshot = if status == 200 {
            result.unwrap()
        } else {
            assert!(matches!(result, Err(Error::NoosnoteConcurrentUpdate)));
            state
                .notes
                .lock()
                .await
                .as_ref()
                .unwrap()
                .snapshot(None, "ready")
        };
        let persisted = load_cache(&state.blobs, 42).unwrap();
        (snapshot, persisted, server.join().unwrap())
    }

    #[tokio::test]
    async fn first_note_creates_one_private_repository_and_commits_related_files_together() {
        let (snapshot, cache, requests) = publish_fixture(200).await;
        assert_eq!(snapshot.dirty_count, 0);
        assert!(cache.index.dirty.is_empty());
        assert_eq!(requests[1].1["private"], true);
        assert_eq!(requests[1].1["name"], "noosnote_42");
        assert_eq!(requests[6].1["tree"].as_array().unwrap().len(), 3);
        assert_eq!(requests[7].1["parents"], json!(["a".repeat(40)]));
        assert_eq!(requests[8].1["force"], false);
    }

    #[tokio::test]
    async fn a_concurrent_github_commit_preserves_every_protected_draft() {
        let (snapshot, cache, requests) = publish_fixture(409).await;
        assert_eq!(snapshot.dirty_count, 3);
        assert_eq!(cache.index.dirty.len(), 3);
        assert!(
            cache
                .documents
                .values()
                .all(|document| document.local.is_some())
        );
        assert_eq!(requests[8].1["force"], false);
    }

    fn blob(hash: &str, value: &Value) -> Value {
        let text = serde_json::to_string(value).unwrap();
        json!({"oid":hash,"byteSize":text.len(),"text":text,"isTruncated":false})
    }

    #[tokio::test]
    async fn another_machine_reuses_the_repository_and_fetches_pages_only_when_opened() {
        let (page_path, page) = page();
        let (workspace_path, workspace) = workspace(&page);
        let marker = json!({"version":1,"ownerId":42});
        let marker_hash = "c".repeat(40);
        let workspace_hash = "d".repeat(40);
        let page_hash = "e".repeat(40);
        let tree = json!({"sha":"b".repeat(40),"truncated":false,"tree":[{"path":MARKER,"sha":marker_hash,"mode":"100644","type":"blob","size":100},{"path":workspace_path,"sha":workspace_hash,"mode":"100644","type":"blob","size":1000},{"path":page_path,"sha":page_hash,"mode":"100644","type":"blob","size":1000}]});
        let (origin, server) = http_fixture(vec![
            ("GET /repos/octocat/noosnote_42", 200, repository()),
            (
                "GET /repos/octocat/noosnote_42/contents/.noosnote.json",
                200,
                json!({"type":"file","encoding":"base64","size":serde_json::to_vec(&marker).unwrap().len(),"content":STANDARD.encode(serde_json::to_vec(&marker).unwrap())}),
            ),
            ("GET /repos/octocat/noosnote_42", 200, repository()),
            (
                "GET /repos/octocat/noosnote_42/git/ref/heads/main",
                200,
                json!({"object":{"sha":"a".repeat(40)}}),
            ),
            (
                "GET /repos/octocat/noosnote_42/git/trees/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa?recursive=1",
                200,
                tree,
            ),
            (
                "POST /graphql",
                200,
                json!({"data":{"repository":{"databaseId":421,"isPrivate":true,"f0":blob(&marker_hash,&marker),"f1":blob(&workspace_hash,&workspace)}}}),
            ),
            (
                "POST /graphql",
                200,
                json!({"data":{"repository":{"databaseId":421,"isPrivate":true,"f0":blob(&page_hash,&page)}}}),
            ),
        ]);
        let temporary = tempfile::tempdir().unwrap();
        let state = state(temporary.path(), origin).await;
        assert_eq!(prepare(&state, false).await.unwrap().setup, "ready");
        let snapshot = sync(&state).await.unwrap();
        assert!(snapshot.files.contains_key(&workspace_path));
        assert!(!snapshot.files.contains_key(&page_path));
        let loaded = load_page(
            &state,
            page["workspaceId"].as_str().unwrap(),
            page["id"].as_str().unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(loaded.files[&page_path].as_ref(), &page);
        let requests = server.join().unwrap();
        assert!(!requests.iter().any(|(path, _)| path == "POST /user/repos"));
        assert!(
            !requests[5].1["query"]
                .as_str()
                .unwrap()
                .contains(&page_hash)
        );
        assert!(
            requests[6].1["query"]
                .as_str()
                .unwrap()
                .contains(&page_hash)
        );
    }
}
