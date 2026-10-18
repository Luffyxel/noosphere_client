use crate::{
    DesktopAppHandle, commands,
    error::{Error, Result},
    models::GitHubViewer,
    noosnote::{self, Cache, Change, Repository, Snapshot},
    state::AppState,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::Method;
use secrecy::{ExposeSecret as _, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use tauri::State;
use uuid::Uuid;

const MARKER: &str = ".noosnote-team.json";
const MAX_MEMBERS: usize = 100;
type CommandResult<T> = std::result::Result<T, String>;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Team {
    pub id: String,
    pub repository_id: u64,
    pub owner_id: u64,
    pub owner: String,
    pub name: String,
    pub branch: String,
    pub title: String,
    pub can_edit: bool,
    pub can_manage: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    id: u64,
    login: String,
    name: String,
    avatar_url: String,
    can_edit: bool,
    owner: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamStatus {
    pub team: Team,
    members: Vec<Person>,
    #[serde(default)]
    activity: Vec<Value>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preparation {
    stage: &'static str,
    team: Option<Team>,
    url: Option<String>,
}

fn valid_id(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
pub fn repository_name(id: &str) -> String {
    format!("noosnote_team_{id}")
}
pub fn is_repository_name(name: &str) -> bool {
    name.strip_prefix("noosnote_team_").is_some_and(valid_id)
}
fn key(owner: u64) -> String {
    format!("noosnote-{owner}-teams")
}
fn root(team: &Team) -> String {
    format!("/repos/{}/{}", team.owner, team.name)
}
fn scope(team: &Team) -> String {
    format!("team-{}", team.repository_id)
}
fn error(error: Error) -> String {
    error.to_string()
}
fn scoped_path(team: &Team, path: &str) -> Result<()> {
    let (_, workspace, _) = noosnote::file_kind(path)?;
    if workspace != team.id {
        return Err(Error::InvalidData);
    }
    Ok(())
}
fn parse_link(input: &str) -> Result<(String, String, String)> {
    let trimmed = input.trim().trim_end_matches('/');
    let value = trimmed
        .strip_prefix("https://github.com/")
        .unwrap_or(trimmed);
    let value = value.split('#').next().unwrap_or(value);
    let mut parts = value.split('/');
    let owner = parts.next().ok_or(Error::InvalidData)?;
    crate::validation::github_login(owner)?;
    let name = parts.next().ok_or(Error::InvalidData)?;
    let id = name
        .strip_prefix("noosnote_team_")
        .filter(|id| valid_id(id))
        .ok_or(Error::InvalidData)?;
    if parts.next().is_some() {
        return Err(Error::InvalidData);
    }
    Ok((owner.into(), name.into(), id.into()))
}
async fn links(state: &AppState, owner: u64) -> Result<Vec<Team>> {
    let store = state.blobs.clone();
    tokio::task::spawn_blocking(move || {
        let Some(data) = store.load(&key(owner))? else {
            return Ok(Vec::new());
        };
        let teams: Vec<Team> = serde_json::from_str(data.expose_secret())
            .map_err(|_| Error::SecureStorageUnavailable)?;
        if teams.len() > 32
            || teams.iter().any(|team| {
                !valid_id(&team.id)
                    || team.repository_id == 0
                    || team.name != repository_name(&team.id)
                    || crate::validation::github_login(&team.owner).is_err()
            })
        {
            return Err(Error::SecureStorageUnavailable);
        }
        Ok(teams)
    })
    .await
    .map_err(|_| Error::Local)?
}
async fn remember(state: &AppState, owner: u64, team: Team) -> Result<()> {
    let _guard = state.teams_registry.lock().await;
    let mut teams = links(state, owner).await?;
    if teams
        .iter()
        .any(|existing| existing.id == team.id && existing.repository_id != team.repository_id)
    {
        return Err(Error::NoosnoteRepositoryMismatch);
    }
    teams.retain(|existing| existing.id != team.id);
    if teams.len() >= 32 {
        return Err(Error::ResponseTooLarge);
    }
    teams.push(team);
    let store = state.blobs.clone();
    tokio::task::spawn_blocking(move || {
        store.save(
            &key(owner),
            SecretString::from(serde_json::to_string(&teams).map_err(|_| Error::Local)?),
        )
    })
    .await
    .map_err(|_| Error::Local)??;
    Ok(())
}
async fn find(state: &AppState, owner: u64, id: &str) -> Result<Team> {
    if !valid_id(id) {
        return Err(Error::InvalidData);
    }
    links(state, owner)
        .await?
        .into_iter()
        .find(|team| team.id == id)
        .ok_or(Error::NoosnotePageUnavailable)
}
async fn cache(state: &AppState, owner: u64, team: &Team) -> Result<Cache> {
    let mut caches = state.team_notes.lock().await;
    let entry = format!("{owner}-{}", team.repository_id);
    if !caches.contains_key(&entry) {
        let store = state.blobs.clone();
        let namespace = scope(team);
        let loaded = tokio::task::spawn_blocking(move || {
            noosnote::load_scoped_cache(&store, owner, Some(namespace))
        })
        .await
        .map_err(|_| Error::Local)??;
        caches.insert(entry.clone(), loaded);
    }
    caches.get(&entry).cloned().ok_or(Error::Local)
}
async fn install(
    state: &AppState,
    owner: u64,
    team: &Team,
    mut cache: Cache,
    changed: BTreeSet<String>,
) -> Result<Snapshot> {
    noosnote::install_cache(state, &mut cache, &changed).await?;
    let snapshot = cache.snapshot(Some(&changed), "ready");
    state
        .team_notes
        .lock()
        .await
        .insert(format!("{owner}-{}", team.repository_id), cache);
    Ok(snapshot)
}
async fn checked(
    state: &AppState,
    token: &SecretString,
    viewer: &GitHubViewer,
    team: Team,
) -> Result<Team> {
    let response =
        noosnote::request(&state.github, token, Method::GET, &root(&team), None, &[]).await?;
    let repository = response.data.ok_or(Error::InvalidGitHubResponse)?;
    repository_access(team, &repository, viewer.id)
}
fn repository_access(mut team: Team, repository: &Value, viewer_id: u64) -> Result<Team> {
    if repository.get("private").and_then(Value::as_bool) != Some(true) {
        return Err(Error::NoosnoteRepositoryPublic);
    }
    if repository.get("id").and_then(Value::as_u64) != Some(team.repository_id)
        || repository.pointer("/owner/id").and_then(Value::as_u64) != Some(team.owner_id)
        || repository.get("name").and_then(Value::as_str) != Some(team.name.as_str())
    {
        return Err(Error::NoosnoteRepositoryMismatch);
    }
    team.branch = noosnote::sha_branch(&repository)?;
    team.can_edit = repository
        .pointer("/permissions/push")
        .and_then(Value::as_bool)
        == Some(true)
        || team.owner_id == viewer_id;
    team.can_manage = repository
        .pointer("/permissions/admin")
        .and_then(Value::as_bool)
        == Some(true)
        || team.owner_id == viewer_id;
    Ok(team)
}
async fn marker(state: &AppState, token: &SecretString, team: &Team) -> Result<()> {
    let response = noosnote::request(
        &state.github,
        token,
        Method::GET,
        &format!("{}/contents/{MARKER}", root(team)),
        None,
        &[],
    )
    .await?;
    let bytes = crate::repository_content::decode_file_bytes(
        response.data.as_ref().ok_or(Error::InvalidGitHubResponse)?,
        8192,
        false,
    )?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidData)?;
    if value["version"] != 1
        || value["workspaceId"] != team.id
        || value["ownerId"] != team.owner_id
        || value["repositoryId"] != team.repository_id
    {
        return Err(Error::NoosnoteRepositoryMismatch);
    }
    Ok(())
}

#[tauri::command]
pub async fn noosnote_teams(state: State<'_, AppState>) -> CommandResult<Vec<Team>> {
    async fn discover(state: &AppState) -> Result<Vec<Team>> {
        let viewer = commands::session_viewer(state).await?;
        let existing = links(state, viewer.id).await?;
        let token = match state.access_token().await {
            Ok(token) => token,
            Err(_) => return Ok(existing),
        };
        let response = match noosnote::request(
            &state.github,
            &token,
            Method::GET,
            "/user/repos?affiliation=owner,collaborator&sort=updated&per_page=100",
            None,
            &[],
        )
        .await
        {
            Ok(value) => value,
            Err(_) => return Ok(existing),
        };
        let entries = response
            .data
            .as_ref()
            .and_then(Value::as_array)
            .ok_or(Error::InvalidGitHubResponse)?;
        for entry in entries.iter().take(100) {
            let Some(name) = entry["name"]
                .as_str()
                .filter(|name| is_repository_name(name))
            else {
                continue;
            };
            if entry["private"] != true {
                continue;
            }
            let Some(owner) = entry
                .pointer("/owner/login")
                .and_then(Value::as_str)
                .filter(|login| crate::validation::github_login(login).is_ok())
            else {
                continue;
            };
            let Some(repository_id) = entry["id"].as_u64().filter(|id| *id > 0) else {
                continue;
            };
            let Some(owner_id) = entry
                .pointer("/owner/id")
                .and_then(Value::as_u64)
                .filter(|id| *id > 0)
            else {
                continue;
            };
            if existing
                .iter()
                .any(|team| team.repository_id == repository_id)
            {
                continue;
            }
            let team = Team {
                id: name.trim_start_matches("noosnote_team_").into(),
                repository_id,
                owner_id,
                owner: owner.into(),
                name: name.into(),
                branch: "main".into(),
                title: entry["description"]
                    .as_str()
                    .unwrap_or("Noosnote partagé")
                    .into(),
                can_edit: false,
                can_manage: false,
            };
            if let Ok(team) = repository_access(team, entry, viewer.id) {
                if marker(state, &token, &team).await.is_ok() {
                    let _ = remember(state, viewer.id, team).await;
                }
            }
        }
        links(state, viewer.id).await
    }
    discover(&state).await.map_err(error)
}

#[tauri::command]
pub async fn noosnote_team_join(state: State<'_, AppState>, link: String) -> CommandResult<Team> {
    async fn join(state: &AppState, link: &str) -> Result<Team> {
        let (owner, name, id) = parse_link(link)?;
        let viewer = commands::session_viewer(state).await?;
        let token = state.access_token().await?;
        let response = noosnote::request(
            &state.github,
            &token,
            Method::GET,
            &format!("/repos/{owner}/{name}"),
            None,
            &[],
        )
        .await?;
        let value = response.data.ok_or(Error::InvalidGitHubResponse)?;
        let team = Team {
            id,
            repository_id: value["id"].as_u64().ok_or(Error::InvalidGitHubResponse)?,
            owner_id: value
                .pointer("/owner/id")
                .and_then(Value::as_u64)
                .ok_or(Error::InvalidGitHubResponse)?,
            owner,
            name,
            branch: "main".into(),
            title: value["description"]
                .as_str()
                .unwrap_or("Noosnote partagé")
                .into(),
            can_edit: false,
            can_manage: false,
        };
        let team = checked(state, &token, &viewer, team).await?;
        marker(state, &token, &team).await?;
        remember(state, viewer.id, team.clone()).await?;
        Ok(team)
    }
    join(&state, &link).await.map_err(error)
}

#[tauri::command]
pub async fn noosnote_team_prepare(
    state: State<'_, AppState>,
    workspace_id: String,
    create: bool,
) -> CommandResult<Preparation> {
    async fn prepare(state: &AppState, id: &str, create: bool) -> Result<Preparation> {
        if !valid_id(id) {
            return Err(Error::InvalidData);
        }
        let viewer = commands::session_viewer(state).await?;
        let token = state.access_token().await?;
        let name = repository_name(id);
        let mut personal = noosnote::cached(state, viewer.id).await?;
        let workspace_path = format!("notes/{id}/workspace.json");
        let workspace = personal
            .documents
            .get(&workspace_path)
            .and_then(|d| d.local.as_deref())
            .ok_or(Error::NoosnotePageUnavailable)?;
        let title = workspace["title"]
            .as_str()
            .unwrap_or("Noosnote partagé")
            .to_owned();
        let root = format!("/repos/{}/{name}", viewer.login);
        let mut response =
            noosnote::request(&state.github, &token, Method::GET, &root, None, &[403, 404]).await?;
        if response.status != 200 && create {
            response = noosnote::request(&state.github, &token, Method::POST, "/user/repos", Some(&json!({"name":name,"description":format!("Noosnote partagé · {title}"),"private":true,"auto_init":true,"has_issues":false,"has_projects":false,"has_wiki":false})), &[403,404,422]).await?;
        }
        if !matches!(response.status, 200 | 201) {
            let url = format!(
                "https://github.com/new?name={}&description={}&visibility=private",
                name,
                noosnote::segment(&format!("Noosnote partagé · {title}"))
            );
            return Ok(Preparation {
                stage: "repository",
                team: None,
                url: Some(url),
            });
        }
        let value = response.data.ok_or(Error::InvalidGitHubResponse)?;
        let team = Team {
            id: id.into(),
            repository_id: value["id"].as_u64().ok_or(Error::InvalidGitHubResponse)?,
            owner_id: viewer.id,
            owner: viewer.login.clone(),
            name,
            branch: "main".into(),
            title,
            can_edit: true,
            can_manage: true,
        };
        let team = checked(state, &token, &viewer, team).await?;
        let access = noosnote::request(
            &state.github,
            &token,
            Method::GET,
            &format!("{root}/contents/{MARKER}"),
            None,
            &[403, 404],
        )
        .await?;
        if access.status == 403 {
            return Ok(Preparation {
                stage: "installation",
                team: None,
                url: Some(format!(
                    "https://github.com/apps/{}/installations/new",
                    crate::github::APP_SLUG
                )),
            });
        }
        if access.status == 404 {
            let metadata = json!({"version":1,"workspaceId":id,"ownerId":viewer.id,"repositoryId":team.repository_id});
            let write = noosnote::request(&state.github, &token, Method::PUT, &format!("{root}/contents/{MARKER}"), Some(&json!({"message":"Initialize shared Noosnote workspace","content":STANDARD.encode(serde_json::to_vec(&metadata).map_err(|_| Error::Local)?),"branch":team.branch})), &[403,404]).await?;
            if matches!(write.status, 403 | 404) {
                return Ok(Preparation {
                    stage: "installation",
                    team: None,
                    url: None,
                });
            }
        } else {
            marker(state, &token, &team).await?;
        }
        remember(state, viewer.id, team.clone()).await?;
        if access.status == 200 {
            synchronize(state, id).await?;
            if cache(state, viewer.id, &team)
                .await?
                .documents
                .get(&workspace_path)
                .is_some_and(|d| d.local.is_some())
            {
                return Ok(Preparation {
                    stage: "ready",
                    team: Some(team),
                    url: None,
                });
            }
        }
        let pages: Vec<String> = workspace["pages"]
            .as_array()
            .ok_or(Error::InvalidData)?
            .iter()
            .filter_map(|p| p["id"].as_str().map(str::to_owned))
            .collect();
        for page_id in pages {
            let snapshot = noosnote::load_page(state, id, &page_id).await?;
            if snapshot
                .files
                .values()
                .any(|page| page.get("attachment").is_some())
            {
                crate::noosnote_files::read_personal_file(state, id, &page_id).await?;
            }
        }
        personal = noosnote::cached(state, viewer.id).await?;
        let mut target = cache(state, viewer.id, &team).await?;
        target.index.repository = Some(Repository {
            id: team.repository_id,
            name: team.name.clone(),
            url: format!("https://github.com/{}/{}", team.owner, team.name),
            branch: team.branch.clone(),
        });
        let mut changes = Vec::new();
        for path in personal
            .index
            .hashes
            .keys()
            .chain(personal.documents.keys())
            .filter(|path| path.starts_with(&format!("notes/{id}/")))
        {
            if noosnote::file_kind(path).is_err()
                || changes.iter().any(|change: &Change| change.path == *path)
            {
                continue;
            }
            let document = if let Some(document) = personal.documents.get(path) {
                document.clone()
            } else if let Some(key) = personal.index.blobs.get(path) {
                noosnote::load_document(&state.blobs, viewer.id, path, key)?
            } else {
                continue;
            };
            if let Some(value) = document.local {
                changes.push(Change {
                    path: path.clone(),
                    value: Some((*value).clone()),
                    base: None,
                });
            }
        }
        let mut changed = BTreeSet::new();
        while !changes.is_empty() {
            let batch: Vec<_> = changes.drain(..changes.len().min(256)).collect();
            changed.extend(noosnote::apply_changes(&mut target, batch)?);
        }
        install(state, viewer.id, &team, target, changed).await?;
        remember(state, viewer.id, team.clone()).await?;
        Ok(Preparation {
            stage: "ready",
            team: Some(team),
            url: None,
        })
    }
    prepare(&state, &workspace_id, create).await.map_err(error)
}
#[tauri::command]
pub async fn noosnote_team_cached(
    state: State<'_, AppState>,
    team_id: String,
) -> CommandResult<Snapshot> {
    async fn cached(state: &AppState, id: &str) -> Result<Snapshot> {
        let viewer = commands::session_viewer(state).await?;
        let team = find(state, viewer.id, id).await?;
        Ok(cache(state, viewer.id, &team)
            .await?
            .snapshot(None, "ready"))
    }
    cached(&state, &team_id).await.map_err(error)
}

fn author(viewer: &GitHubViewer) -> Value {
    json!({"id":viewer.id,"login":viewer.login,"name":viewer.name.as_deref().unwrap_or(&viewer.login),"avatarUrl":viewer.avatar_url})
}
fn protect_thread(
    change: &mut Change,
    previous: Option<&Value>,
    viewer: &GitHubViewer,
    manage: bool,
) -> Result<()> {
    let Some(value) = change.value.as_mut() else {
        if !manage
            && previous
                .and_then(|old| old.pointer("/comments/0/author/id"))
                .and_then(Value::as_u64)
                != Some(viewer.id)
        {
            return Err(Error::GitHubDenied);
        }
        return Ok(());
    };
    if value["kind"] != "thread" {
        return Ok(());
    }
    let comments = value["comments"].as_array_mut().ok_or(Error::InvalidData)?;
    if let Some(old) = previous.and_then(|p| p["comments"].as_array()) {
        for entry in old {
            if !comments.iter().any(|c| c["id"] == entry["id"]) {
                comments.push(entry.clone());
            }
        }
    }
    for comment in comments {
        let old = previous
            .and_then(|p| p["comments"].as_array())
            .and_then(|entries| entries.iter().find(|entry| entry["id"] == comment["id"]));
        if let Some(old) = old {
            if old["body"] != comment["body"] || old.get("deletedAt") != comment.get("deletedAt") {
                if !manage && old.pointer("/author/id").and_then(Value::as_u64) != Some(viewer.id) {
                    return Err(Error::GitHubDenied);
                }
                comment["author"] = old["author"].clone();
                comment["createdAt"] = old["createdAt"].clone();
                comment["editedAt"] = json!(noosnote::now());
            } else if old["author"] != comment["author"] || old["createdAt"] != comment["createdAt"]
            {
                return Err(Error::GitHubDenied);
            }
        } else {
            if comment.pointer("/author/id").and_then(Value::as_u64) != Some(viewer.id) {
                return Err(Error::GitHubDenied);
            }
            comment["author"] = author(viewer);
            comment["createdAt"] = json!(noosnote::now());
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn noosnote_team_save(
    state: State<'_, AppState>,
    team_id: String,
    changes: Vec<Change>,
) -> CommandResult<Snapshot> {
    async fn save(state: &AppState, id: &str, mut changes: Vec<Change>) -> Result<Snapshot> {
        let viewer = commands::session_viewer(state).await?;
        let team = find(state, viewer.id, id).await?;
        if !team.can_edit {
            return Err(Error::GitHubDenied);
        }
        let _guard = state.teams_edit.lock().await;
        let mut current = cache(state, viewer.id, &team).await?;
        for change in &mut changes {
            scoped_path(&team, &change.path)?;
            if change.path.contains("/threads/") {
                let previous = current
                    .documents
                    .get(&change.path)
                    .and_then(|doc| doc.local.as_deref());
                protect_thread(change, previous, &viewer, team.can_manage)?;
            }
            if change.path.ends_with("/workspace.json") {
                if change.value.is_none() && !team.can_manage {
                    return Err(Error::GitHubDenied);
                }
                if let Some(value) = change.value.as_mut() {
                    let own = viewer.id.to_string();
                    let requested = value
                        .get("profiles")
                        .and_then(Value::as_object)
                        .and_then(|p| p.get(&own))
                        .cloned();
                    let old = current
                        .documents
                        .get(&change.path)
                        .and_then(|d| d.local.as_deref())
                        .and_then(|v| v.get("profiles"))
                        .and_then(Value::as_object);
                    let mut profiles = old.cloned().unwrap_or_default();
                    if let Some(name) = requested {
                        profiles.insert(own, name);
                    }
                    if !profiles.is_empty() {
                        value["profiles"] = Value::Object(profiles);
                    }
                }
            }
        }

        let changed = noosnote::apply_changes(&mut current, changes)?;
        install(state, viewer.id, &team, current, changed).await
    }
    save(&state, &team_id, changes).await.map_err(error)
}

fn merge_documents(
    cache: &mut Cache,
    values: BTreeMap<String, Option<Arc<Value>>>,
    hashes: BTreeMap<String, String>,
    head: String,
) -> Result<BTreeSet<String>> {
    let mut changed = BTreeSet::new();
    for (path, remote) in values {
        if let Some(value) = &remote {
            noosnote::validate_file(&path, value)?;
        }
        let document = cache
            .documents
            .entry(path.clone())
            .or_insert_with(|| noosnote::Document {
                owner_id: cache.index.owner_id,
                path: path.clone(),
                local: None,
                base: None,
                base_sha: None,
                conflict: None,
            });
        match noosnote::merge_value(
            document.base.as_deref(),
            document.local.as_deref(),
            remote.as_deref(),
            0,
        ) {
            Ok(value)
                if value
                    .as_ref()
                    .is_none_or(|value| noosnote::validate_file(&path, value).is_ok()) =>
            {
                document.local = value.map(Arc::new);
                document.conflict = None;
            }
            _ => {
                document.conflict = Some(noosnote::Conflict {
                    id: Uuid::new_v4().to_string(),
                    path: path.clone(),
                    local: document.local.clone(),
                    remote: remote.clone(),
                });
            }
        }
        document.base = remote;
        document.base_sha = hashes.get(&path).cloned();
        if document.local != document.base || document.conflict.is_some() {
            cache.index.dirty.insert(path.clone());
        } else {
            cache.index.dirty.remove(&path);
        }
        changed.insert(path);
    }
    cache.index.hashes = hashes;
    cache.index.head = Some(head);
    cache.index.last_synced_at = Some(noosnote::now());
    Ok(changed)
}

async fn synchronize(state: &AppState, id: &str) -> Result<Snapshot> {
    let _sync = state.teams_sync.lock().await;
    let viewer = commands::session_viewer(state).await?;
    let token = state.access_token().await?;
    let team = checked(state, &token, &viewer, find(state, viewer.id, id).await?).await?;
    marker(state, &token, &team).await?;
    remember(state, viewer.id, team.clone()).await?;
    let root = root(&team);
    for attempt in 0..3 {
        let initial = cache(state, viewer.id, &team).await?;
        let reference = noosnote::request(
            &state.github,
            &token,
            Method::GET,
            &format!("{root}/git/ref/heads/{}", noosnote::segment(&team.branch)),
            None,
            &[],
        )
        .await?;
        let head = noosnote::sha(
            reference
                .data
                .as_ref()
                .and_then(|v| v.get("object"))
                .ok_or(Error::InvalidGitHubResponse)?,
            "sha",
        )?;
        if initial.index.head.as_deref() == Some(&head) && initial.index.dirty.is_empty() {
            return Ok(initial.snapshot(Some(&BTreeSet::new()), "ready"));
        }
        let tree_response = noosnote::request(
            &state.github,
            &token,
            Method::GET,
            &format!("{root}/git/trees/{head}?recursive=1"),
            None,
            &[],
        )
        .await?;
        let tree = tree_response.data.ok_or(Error::InvalidGitHubResponse)?;
        if tree["truncated"] != false {
            return Err(Error::ResponseTooLarge);
        }
        let entries = tree["tree"]
            .as_array()
            .filter(|entries| entries.len() <= 30_000)
            .ok_or(Error::ResponseTooLarge)?;
        let mut hashes = BTreeMap::new();
        let mut sizes = BTreeMap::new();
        let mut remote_files = BTreeMap::new();
        for entry in entries {
            let Some(path) = entry["path"].as_str() else {
                return Err(Error::InvalidGitHubResponse);
            };
            if entry["type"] != "blob" || entry["mode"] != "100644" {
                continue;
            }
            let hash = noosnote::sha(entry, "sha")?;
            remote_files.insert(path.to_owned(), hash.clone());
            if scoped_path(&team, path).is_err() {
                continue;
            }
            let size = entry["size"]
                .as_u64()
                .filter(|size| *size <= 1024 * 1024)
                .ok_or(Error::ResponseTooLarge)?;
            hashes.insert(path.to_owned(), hash);
            sizes.insert(path.to_owned(), size);
        }
        if hashes.len() > 8192 {
            return Err(Error::ResponseTooLarge);
        }
        let repository = Repository {
            id: team.repository_id,
            name: team.name.clone(),
            url: format!("https://github.com/{}/{}", team.owner, team.name),
            branch: team.branch.clone(),
        };
        let mut remote_viewer = viewer.clone();
        remote_viewer.login = team.owner.clone();
        let previous: BTreeMap<_, _> = initial
            .documents
            .iter()
            .filter_map(|(path, d)| d.base_sha.as_ref().map(|sha| (path.clone(), sha.clone())))
            .collect();
        let mut values = noosnote::read_documents(
            state,
            &token,
            &remote_viewer,
            &repository,
            &hashes,
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
        let _edit = state.teams_edit.lock().await;
        let mut latest = cache(state, viewer.id, &team).await?;
        latest.index.repository = Some(repository);
        let changed = merge_documents(&mut latest, values, hashes, head.clone())?;
        let snapshot = install(state, viewer.id, &team, latest.clone(), changed).await?;
        drop(_edit);
        if latest.index.dirty.is_empty()
            || latest.documents.values().any(|d| d.conflict.is_some())
            || !team.can_edit
        {
            return Ok(snapshot);
        }
        let mut updates = noosnote::commit_tree(&latest)?;
        let pages: Vec<_> = latest
            .index
            .dirty
            .iter()
            .filter_map(|path| {
                latest
                    .documents
                    .get(path)?
                    .local
                    .as_deref()
                    .filter(|v| v.get("attachment").is_some())
                    .cloned()
            })
            .collect();
        updates.extend(
            crate::noosnote_files::publish_chunks(state, &root, &token, &pages, &remote_files)
                .await?,
        );
        let tree_response = noosnote::request(
            &state.github,
            &token,
            Method::POST,
            &format!("{root}/git/trees"),
            Some(&json!({"base_tree":noosnote::sha(&tree,"sha")?,"tree":updates})),
            &[],
        )
        .await?;
        let commit = noosnote::request(&state.github, &token, Method::POST, &format!("{root}/git/commits"), Some(&json!({"message":"Update shared Noosnote workspace","tree":noosnote::sha(tree_response.data.as_ref().ok_or(Error::InvalidGitHubResponse)?,"sha")?,"parents":[head]})), &[]).await?;
        let commit_sha = noosnote::sha(
            commit.data.as_ref().ok_or(Error::InvalidGitHubResponse)?,
            "sha",
        )?;
        let published = noosnote::request(
            &state.github,
            &token,
            Method::PATCH,
            &format!("{root}/git/refs/heads/{}", noosnote::segment(&team.branch)),
            Some(&json!({"sha":commit_sha,"force":false})),
            &[409, 422],
        )
        .await?;
        if published.status != 200 {
            if attempt == 2 {
                return Err(Error::NoosnoteConflict);
            }
            continue;
        }
        let _edit = state.teams_edit.lock().await;
        let mut current = cache(state, viewer.id, &team).await?;
        let changed = noosnote::record_publish(&mut current, &latest)?;
        current.index.head = Some(commit_sha);
        return install(state, viewer.id, &team, current, changed).await;
    }
    Err(Error::NoosnoteConflict)
}

#[tauri::command]
pub async fn noosnote_team_sync(
    state: State<'_, AppState>,
    team_id: String,
) -> CommandResult<Snapshot> {
    synchronize(&state, &team_id).await.map_err(error)
}

#[tauri::command]
pub async fn noosnote_team_page(
    state: State<'_, AppState>,
    team_id: String,
    workspace_id: String,
    page_id: String,
) -> CommandResult<Snapshot> {
    async fn page(state: &AppState, id: &str, workspace: &str, page: &str) -> Result<Snapshot> {
        if workspace != id || !valid_id(page) {
            return Err(Error::InvalidData);
        }
        let viewer = commands::session_viewer(state).await?;
        let team = find(state, viewer.id, id).await?;
        let cache = cache(state, viewer.id, &team).await?;
        let path = format!("notes/{id}/pages/{page}.json");
        if !cache.documents.contains_key(&path) {
            return synchronize(state, id).await;
        }
        Ok(cache.snapshot(Some(&BTreeSet::from([path])), "ready"))
    }
    page(&state, &team_id, &workspace_id, &page_id)
        .await
        .map_err(error)
}

#[tauri::command]
pub async fn noosnote_team_resolve(
    state: State<'_, AppState>,
    team_id: String,
    conflict_id: String,
    choice: String,
) -> CommandResult<Snapshot> {
    async fn resolve(
        state: &AppState,
        id: &str,
        conflict_id: &str,
        choice: &str,
    ) -> Result<Snapshot> {
        if !matches!(choice, "local" | "remote") {
            return Err(Error::InvalidData);
        }
        let viewer = commands::session_viewer(state).await?;
        let team = find(state, viewer.id, id).await?;
        if !team.can_edit {
            return Err(Error::GitHubDenied);
        }
        let _edit = state.teams_edit.lock().await;
        let mut cache = cache(state, viewer.id, &team).await?;
        let (path, document) = cache
            .documents
            .iter_mut()
            .find(|(_, d)| d.conflict.as_ref().is_some_and(|c| c.id == conflict_id))
            .ok_or(Error::InvalidData)?;
        let conflict = document.conflict.take().ok_or(Error::InvalidData)?;
        state.blobs.save(
            &format!("nn-team-recovery-{conflict_id}"),
            SecretString::from(serde_json::to_string(&conflict).map_err(|_| Error::Local)?),
        )?;
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
        install(state, viewer.id, &team, cache, changed).await
    }
    resolve(&state, &team_id, &conflict_id, &choice)
        .await
        .map_err(error)
}

#[tauri::command]
pub async fn noosnote_team_status(
    state: State<'_, AppState>,
    team_id: String,
) -> CommandResult<TeamStatus> {
    async fn status(state: &AppState, id: &str) -> Result<TeamStatus> {
        let viewer = commands::session_viewer(state).await?;
        let token = state.access_token().await?;
        let team = checked(state, &token, &viewer, find(state, viewer.id, id).await?).await?;
        remember(state, viewer.id, team.clone()).await?;
        let response = noosnote::request(
            &state.github,
            &token,
            Method::GET,
            &format!("{}/collaborators?per_page={MAX_MEMBERS}", root(&team)),
            None,
            &[403, 404],
        )
        .await?;
        let mut members = Vec::new();
        if response.status == 200 {
            for value in response
                .data
                .as_ref()
                .and_then(Value::as_array)
                .ok_or(Error::InvalidGitHubResponse)?
            {
                let id = value["id"]
                    .as_u64()
                    .filter(|id| *id > 0)
                    .ok_or(Error::InvalidGitHubResponse)?;
                let login = value["login"]
                    .as_str()
                    .ok_or(Error::InvalidGitHubResponse)?;
                crate::validation::github_login(login)?;
                members.push(Person {
                    id,
                    login: login.into(),
                    name: login.into(),
                    avatar_url: format!("https://avatars.githubusercontent.com/u/{id}"),
                    can_edit: value.pointer("/permissions/push").and_then(Value::as_bool)
                        == Some(true)
                        || id == team.owner_id,
                    owner: id == team.owner_id,
                });
            }
        }
        if !members.iter().any(|p| p.id == viewer.id) {
            members.push(Person {
                id: viewer.id,
                login: viewer.login.clone(),
                name: viewer.name.clone().unwrap_or(viewer.login.clone()),
                avatar_url: viewer.avatar_url.clone(),
                can_edit: team.can_edit,
                owner: viewer.id == team.owner_id,
            });
        }
        Ok(TeamStatus {
            team,
            members,
            activity: Vec::new(),
        })
    }
    status(&state, &team_id).await.map_err(error)
}

#[tauri::command]
pub async fn noosnote_team_open(
    app: DesktopAppHandle,
    state: State<'_, AppState>,
    team_id: String,
    mode: String,
) -> CommandResult<()> {
    async fn open(app: &DesktopAppHandle, state: &AppState, id: &str, mode: &str) -> Result<()> {
        if !valid_id(id) {
            return Err(Error::InvalidData);
        }
        let viewer = commands::session_viewer(state).await?;
        let destination = if mode == "setup-repository" {
            let personal = noosnote::cached(state, viewer.id).await?;
            if !personal
                .documents
                .get(&format!("notes/{id}/workspace.json"))
                .is_some_and(|d| d.local.is_some())
            {
                return Err(Error::GitHubDenied);
            }
            format!(
                "https://github.com/new?name={}&visibility=private&auto_init=true",
                repository_name(id)
            )
        } else if mode == "setup-installation" || mode == "installation" {
            format!(
                "https://github.com/apps/{}/installations/new",
                crate::github::APP_SLUG
            )
        } else {
            let team = find(state, viewer.id, id).await?;
            let suffix = if let Some(commit) = mode.strip_prefix("revision:") {
                if !matches!(commit.len(), 40 | 64)
                    || !commit.bytes().all(|c| c.is_ascii_hexdigit())
                {
                    return Err(Error::InvalidData);
                }
                format!("/commit/{commit}")
            } else {
                match mode {
                    "repository" => String::new(),
                    "access" if team.can_manage => "/settings/access".into(),
                    _ => return Err(Error::GitHubDenied),
                }
            };
            format!("https://github.com/{}/{}{suffix}", team.owner, team.name)
        };
        commands::open_github_url(
            app,
            &url::Url::parse(&destination).map_err(|_| Error::InvalidData)?,
        )
        .await
    }
    open(&app, &state, &team_id, &mode).await.map_err(error)
}

#[tauri::command]
pub async fn noosnote_team_history(
    state: State<'_, AppState>,
    team_id: String,
    page_id: Option<String>,
) -> CommandResult<Vec<Value>> {
    async fn history(state: &AppState, id: &str, page: Option<String>) -> Result<Vec<Value>> {
        let viewer = commands::session_viewer(state).await?;
        let token = state.access_token().await?;
        let team = checked(state, &token, &viewer, find(state, viewer.id, id).await?).await?;
        let path = if let Some(page) = page {
            if !valid_id(&page) {
                return Err(Error::InvalidData);
            }
            format!("notes/{id}/pages/{page}.json")
        } else {
            format!("notes/{id}")
        };
        let response = noosnote::request(
            &state.github,
            &token,
            Method::GET,
            &format!(
                "{}/commits?per_page=30&sha={}&path={}",
                root(&team),
                noosnote::segment(&team.branch),
                noosnote::segment(&path)
            ),
            None,
            &[],
        )
        .await?;
        let entries = response
            .data
            .as_ref()
            .and_then(Value::as_array)
            .filter(|v| v.len() <= 30)
            .ok_or(Error::InvalidGitHubResponse)?;
        entries.iter().map(|entry| {
            let sha=noosnote::sha(entry,"sha")?;
            Ok(json!({"id":sha,"url":format!("https://github.com/{}/{}/commit/{sha}",team.owner,team.name),"author":entry.get("author"),"name":entry.pointer("/commit/author/name"),"date":entry.pointer("/commit/author/date"),"message":entry.pointer("/commit/message")}))
        }).collect()
    }
    history(&state, &team_id, page_id).await.map_err(error)
}

#[tauri::command]
pub async fn noosnote_team_read_file(
    state: State<'_, AppState>,
    team_id: String,
    workspace_id: String,
    page_id: String,
) -> CommandResult<String> {
    async fn read(state: &AppState, id: &str, workspace: &str, page: &str) -> Result<String> {
        if workspace != id || !valid_id(page) {
            return Err(Error::InvalidData);
        }
        let viewer = commands::session_viewer(state).await?;
        let team = find(state, viewer.id, id).await?;
        let cached = cache(state, viewer.id, &team).await?;
        let value = cached
            .documents
            .get(&format!("notes/{id}/pages/{page}.json"))
            .and_then(|d| d.local.as_deref())
            .ok_or(Error::NoosnotePageUnavailable)?;
        crate::noosnote_files::read_cached_file(state, value, &root(&team), team.repository_id)
            .await
    }
    read(&state, &team_id, &workspace_id, &page_id)
        .await
        .map_err(error)
}

#[tauri::command]
pub async fn noosnote_team_leave(state: State<'_, AppState>, team_id: String) -> CommandResult<()> {
    async fn leave(state: &AppState, id: &str) -> Result<()> {
        let viewer = commands::session_viewer(state).await?;
        let team = find(state, viewer.id, id).await?;
        if !cache(state, viewer.id, &team).await?.index.dirty.is_empty() {
            return Err(Error::NoosnoteConcurrentUpdate);
        }
        let _registry = state.teams_registry.lock().await;
        let mut teams = links(state, viewer.id).await?;
        teams.retain(|team| team.id != id);
        state.blobs.save(
            &key(viewer.id),
            SecretString::from(serde_json::to_string(&teams).map_err(|_| Error::Local)?),
        )?;
        state
            .team_notes
            .lock()
            .await
            .remove(&format!("{}-{}", viewer.id, team.repository_id));
        Ok(())
    }
    leave(&state, &team_id).await.map_err(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn team_links_only_accept_dedicated_repository_names() {
        let id = "e6470549-cceb-40bd-a5c8-1647a6a0aec3";
        assert!(parse_link(&format!("https://github.com/alice/noosnote_team_{id}")).is_ok());
        for link in [
            "https://evil.test/alice/noosnote_team_bad",
            "alice/noosnote_42",
            "alice/noosnote_team_../private",
            "alice/noosnote_team_e6470549-cceb-40bd-a5c8-1647a6a0aec3/../secret",
        ] {
            assert!(parse_link(link).is_err());
        }
    }
    #[test]
    fn concurrent_collaboration_states_are_kept_together() {
        let base = json!({"kind":"page","collaborationUpdates":["AQ=="],"content":{"type":"doc"}});
        let mut local = base.clone();
        local["collaborationUpdates"] = json!(["Ag=="]);
        local["content"] = json!({"type":"doc","text":"Alice"});
        let mut remote = base.clone();
        remote["collaborationUpdates"] = json!(["Aw=="]);
        remote["content"] = json!({"type":"doc","text":"Bob"});
        let merged = noosnote::merge_value(Some(&base), Some(&local), Some(&remote), 0)
            .unwrap()
            .unwrap();
        assert_eq!(merged["collaborationUpdates"].as_array().unwrap().len(), 2);
    }
    fn team() -> Team {
        Team {
            id: "0f4fbbe5-3051-4944-a537-0342c58250ad".into(),
            repository_id: 100,
            owner_id: 1,
            owner: "alice".into(),
            name: "noosnote_team_0f4fbbe5-3051-4944-a537-0342c58250ad".into(),
            branch: "main".into(),
            title: "Atelier".into(),
            can_edit: false,
            can_manage: false,
        }
    }
    fn viewer(id: u64) -> GitHubViewer {
        GitHubViewer {
            id,
            login: format!("person{id}"),
            name: None,
            avatar_url: String::new(),
            repository: crate::models::RepositorySummary {
                id: 50,
                name: "noosphere".into(),
                url: "https://github.com/alice/noosphere".into(),
                created: false,
            },
        }
    }
    #[test]
    fn access_comes_from_private_repository_permissions() {
        let team = team();
        let repository = json!({"private":true,"id":100,"name":team.name,"owner":{"id":1},"default_branch":"main","permissions":{"push":false,"admin":false}});
        assert!(
            !repository_access(team.clone(), &repository, 2)
                .unwrap()
                .can_edit
        );
        assert!(
            repository_access(team.clone(), &repository, 1)
                .unwrap()
                .can_manage
        );
        let mut write = repository.clone();
        write["permissions"]["push"] = json!(true);
        assert!(repository_access(team.clone(), &write, 2).unwrap().can_edit);
        for field in ["private", "id", "owner"] {
            let mut invalid = repository.clone();
            invalid[field] = Value::Null;
            assert!(repository_access(team.clone(), &invalid, 2).is_err());
        }
        let mut invalid = repository;
        invalid["name"] = json!("personal-notes");
        assert!(repository_access(team, &invalid, 2).is_err());
    }
    #[test]
    fn comments_cannot_impersonate_or_overwrite_another_member() {
        let existing = json!({"kind":"thread","comments":[{"id":"first","body":"Bonjour","author":{"id":1,"login":"alice","name":"Alice","avatarUrl":""},"createdAt":"2026-10-18T09:00:00Z"}]});
        let mut forged = existing.clone();
        forged["comments"][0]["body"] = json!("Faux message");
        let mut change = Change {
            path: String::new(),
            value: Some(forged),
            base: None,
        };
        assert!(protect_thread(&mut change, Some(&existing), &viewer(2), false).is_err());
        let mut own = json!({"kind":"thread","comments":[{"id":"second","body":"Salut","author":{"id":2,"login":"fake"},"createdAt":"fake"}]});
        let mut change = Change {
            path: String::new(),
            value: Some(own.clone()),
            base: None,
        };
        protect_thread(&mut change, Some(&existing), &viewer(2), false).unwrap();
        own = change.value.unwrap();
        assert_eq!(own["comments"].as_array().unwrap().len(), 2);
        assert_eq!(own["comments"][0]["author"]["login"], "person2");
        assert_ne!(own["comments"][0]["createdAt"], "fake");
    }
    #[tokio::test]
    async fn team_cache_survives_restart_without_touching_personal_notes() {
        let directory = tempfile::tempdir().unwrap();
        let state = AppState::new(
            crate::instance_profile::InstanceProfile::acquire(directory.path()).unwrap(),
        )
        .unwrap();
        let team = team();
        let id = "d02523d7-7538-4848-ae47-4baef0c97f53";
        let path = format!("notes/{}/pages/{id}.json", team.id);
        let page = json!({"version":1,"kind":"page","id":id,"workspaceId":team.id,"title":"Test","icon":"","content":{"type":"doc","content":[{"type":"paragraph"}]}});
        let mut shared = cache(&state, 2, &team).await.unwrap();
        let changes = noosnote::apply_changes(
            &mut shared,
            vec![Change {
                path: path.clone(),
                value: Some(page.clone()),
                base: None,
            }],
        )
        .unwrap();
        install(&state, 2, &team, shared, changes).await.unwrap();
        let restored = noosnote::load_scoped_cache(&state.blobs, 2, Some(scope(&team))).unwrap();
        assert_eq!(restored.documents[&path].local.as_deref(), Some(&page));
        assert!(
            noosnote::load_scoped_cache(&state.blobs, 2, None)
                .unwrap()
                .documents
                .is_empty()
        );
        assert!(
            noosnote::load_scoped_cache(&state.blobs, 3, Some(scope(&team)))
                .unwrap()
                .documents
                .is_empty()
        );
    }
}
