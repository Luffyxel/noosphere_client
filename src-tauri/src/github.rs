use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rand::Rng as _;
use reqwest::{Client, Method, Response, header};
use secrecy::{ExposeSecret as _, SecretString};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};
use tokio::{
    sync::Mutex,
    time::{Instant, sleep},
};
use url::Url;

use crate::{
    error::{Error, Result},
    validation,
};

const API_ORIGIN: &str = "https://api.github.com";
const LOGIN_ORIGIN: &str = "https://github.com";
const MAX_API_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MAX_TOKEN_BYTES: usize = 4_096;

pub const CLIENT_ID: &str = "Iv23liJ0O7tSu2HcfAlD";
pub const APP_SLUG: &str = "noosphere-app";
pub const REPOSITORY_DESCRIPTION: &str = "Données chiffrées Noosphere";

pub struct DeviceCode {
    pub secret: SecretString,
    pub user_code: String,
    pub verification_uri: Url,
    pub interval: Duration,
    pub expires_in: Duration,
}

pub struct OAuthToken {
    pub access_token: SecretString,
    pub refresh_token: Option<SecretString>,
    pub expires_in: Option<Duration>,
    pub refresh_expires_in: Option<Duration>,
}

pub enum OAuthPoll {
    Granted(OAuthToken),
    Pending,
    SlowDown,
    Denied,
    Expired,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewerIdentity {
    pub id: u64,
    pub login: String,
    pub avatar_url: String,
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepositoryIdentity {
    pub id: u64,
    pub name: String,
    pub url: String,
    pub description: Option<String>,
}

pub struct ApiResponse<T> {
    pub status: u16,
    pub etag: Option<String>,
    pub data: Option<T>,
}

#[derive(Clone)]
pub struct GitHubClient {
    client: Client,
    budget: Arc<Mutex<RequestBudget>>,
    cache: Arc<Mutex<ResponseCache>>,
    #[cfg(test)]
    pub(crate) test_origin: Option<Url>,
}

#[derive(Clone)]
struct CachedResponse {
    etag: String,
    bytes: Arc<[u8]>,
    used_at: Instant,
}

#[derive(Default)]
struct ResponseCache {
    entries: BTreeMap<String, CachedResponse>,
    size: usize,
}

impl ResponseCache {
    fn get(&mut self, key: &str) -> Option<CachedResponse> {
        let entry = self.entries.get_mut(key)?;
        entry.used_at = Instant::now();
        Some(entry.clone())
    }

    fn insert(&mut self, key: String, etag: String, bytes: Vec<u8>) {
        if let Some(old) = self.entries.remove(&key) {
            self.size -= old.bytes.len();
        }
        if bytes.len() > 1024 * 1024 {
            return;
        }
        while self.entries.len() >= 64 || self.size + bytes.len() > 16 * 1024 * 1024 {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, value)| value.used_at)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some(old) = self.entries.remove(&oldest) {
                self.size -= old.bytes.len();
            }
        }
        self.size += bytes.len();
        self.entries.insert(
            key,
            CachedResponse {
                etag,
                bytes: bytes.into(),
                used_at: Instant::now(),
            },
        );
    }
}

fn response_cache_key(url: &Url, accept: &str, token: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(token.as_bytes());
    hash.update([0]);
    hash.update(accept.as_bytes());
    hash.update([0]);
    hash.update(url.as_str().as_bytes());
    hex::encode(hash.finalize())
}

#[derive(Default)]
struct RequestBudget {
    next_request: Option<Instant>,
    next_write: Option<Instant>,
    cooldown: Option<Instant>,
    requests: u64,
    writes: u64,
    unchanged: u64,
}

impl GitHubClient {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("Noosphere/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| Error::Network)?;
        Ok(Self {
            client,
            budget: Arc::new(Mutex::new(RequestBudget::default())),
            cache: Arc::new(Mutex::new(ResponseCache::default())),
            #[cfg(test)]
            test_origin: None,
        })
    }

    pub async fn api_json<T: DeserializeOwned>(
        &self,
        method: Method,
        endpoint: &str,
        access_token: Option<&str>,
        etag: Option<&str>,
        body: Option<&serde_json::Value>,
        allowed_statuses: &[u16],
    ) -> Result<ApiResponse<T>> {
        self.api_json_with_accept(
            method,
            endpoint,
            access_token,
            etag,
            body,
            allowed_statuses,
            "application/vnd.github+json",
        )
        .await
    }

    pub async fn api_stargazers<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        access_token: &str,
        etag: Option<&str>,
        allowed_statuses: &[u16],
    ) -> Result<ApiResponse<T>> {
        self.api_json_with_accept(
            Method::GET,
            endpoint,
            Some(access_token),
            etag,
            None,
            allowed_statuses,
            "application/vnd.github.star+json",
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn api_json_with_accept<T: DeserializeOwned>(
        &self,
        method: Method,
        endpoint: &str,
        access_token: Option<&str>,
        etag: Option<&str>,
        body: Option<&serde_json::Value>,
        allowed_statuses: &[u16],
        accept: &'static str,
    ) -> Result<ApiResponse<T>> {
        let url = fixed_origin_url(API_ORIGIN, endpoint)?;
        #[cfg(test)]
        let url = self
            .test_origin
            .as_ref()
            .map(|origin| origin.join(endpoint))
            .transpose()
            .map_err(|_| Error::InvalidData)?
            .unwrap_or(url);
        if let Some(token) = access_token {
            validate_token(token)?;
        }
        self.send_api_json(
            method,
            url,
            access_token,
            etag,
            body,
            allowed_statuses,
            accept,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_api_json<T: DeserializeOwned>(
        &self,
        method: Method,
        url: Url,
        access_token: Option<&str>,
        etag: Option<&str>,
        body: Option<&serde_json::Value>,
        allowed_statuses: &[u16],
        accept: &'static str,
    ) -> Result<ApiResponse<T>> {
        let read_only = method == Method::GET
            || (method == Method::POST
                && url.path() == "/graphql"
                && body
                    .and_then(|body| body.get("query"))
                    .and_then(Value::as_str)
                    .is_some_and(|query| query.trim_start().starts_with("query ")));
        let cache_key = access_token
            .filter(|_| method == Method::GET && body.is_none())
            .map(|token| response_cache_key(&url, accept, token));
        let cached = if etag.is_none() {
            match &cache_key {
                Some(key) => self.cache.lock().await.get(key),
                None => None,
            }
        } else {
            None
        };
        for attempt in 0..=3 {
            self.reserve_request(if read_only { &Method::GET } else { &method })
                .await?;
            let mut request = self
                .client
                .request(method.clone(), url.clone())
                .header(header::ACCEPT, accept)
                .header("X-GitHub-Api-Version", "2022-11-28");
            if let Some(token) = access_token {
                request = request.bearer_auth(token);
            }
            if let Some(value) = etag.or_else(|| cached.as_ref().map(|value| value.etag.as_str())) {
                request = request.header(header::IF_NONE_MATCH, value);
            }
            if let Some(value) = body {
                request = request.json(value);
            }
            let response = match request.send().await {
                Ok(response) => response,
                Err(_) if attempt < 3 => {
                    sleep(retry_delay(attempt)).await;
                    continue;
                }
                Err(_) => return Err(Error::Network),
            };
            let status = response.status().as_u16();
            let (requests, writes, unchanged) = {
                let mut budget = self.budget.lock().await;
                budget.requests = budget.requests.saturating_add(1);
                if !read_only {
                    budget.writes = budget.writes.saturating_add(1);
                }
                if status == 304 {
                    budget.unchanged = budget.unchanged.saturating_add(1);
                }
                (budget.requests, budget.writes, budget.unchanged)
            };
            if let Some(delay) = rate_limit_delay(status, response.headers(), SystemTime::now()) {
                self.budget.lock().await.cooldown = Some(Instant::now() + delay);
                crate::startup::write_log(format_args!(
                    "[github] rateLimit status={status} retryAfterSeconds={} remaining={:?} resetAt={:?} requests={requests} writes={writes} unchanged={unchanged}",
                    delay.as_secs(),
                    response
                        .headers()
                        .get("x-ratelimit-remaining")
                        .and_then(|value| value.to_str().ok())
                        .and_then(|value| value.parse::<u64>().ok()),
                    response
                        .headers()
                        .get("x-ratelimit-reset")
                        .and_then(|value| value.to_str().ok())
                        .and_then(|value| value.parse::<u64>().ok())
                ));
                return Err(Error::RateLimited);
            }
            let etag = response
                .headers()
                .get(header::ETAG)
                .and_then(|value| value.to_str().ok())
                .filter(|value| value.len() <= 512)
                .map(ToOwned::to_owned);
            if matches!(status, 429 | 502 | 503 | 504) && attempt < 3 {
                consume_bounded(response, 64 * 1024).await?;
                sleep(retry_delay(attempt)).await;
                continue;
            }
            if status == 304 || status == 404 || allowed_statuses.contains(&status) {
                consume_bounded(response, 64 * 1024).await?;
                if status == 304
                    && let Some(cached) = &cached
                {
                    let data = if cached.bytes.is_empty() {
                        None
                    } else {
                        Some(
                            serde_json::from_slice(&cached.bytes)
                                .map_err(|_| Error::InvalidGitHubResponse)?,
                        )
                    };
                    return Ok(ApiResponse {
                        status: 200,
                        etag: Some(cached.etag.clone()),
                        data,
                    });
                }
                if status != 304
                    && let Some(key) = &cache_key
                {
                    let mut cache = self.cache.lock().await;
                    if let Some(old) = cache.entries.remove(key) {
                        cache.size -= old.bytes.len();
                    }
                }
                return Ok(ApiResponse {
                    status,
                    etag,
                    data: None,
                });
            }
            if status == 401 {
                return Err(Error::SessionExpired);
            }
            if status == 403 {
                return Err(Error::GitHubDenied);
            }
            if matches!(status, 409 | 422) {
                return Err(Error::GitHubConflict);
            }
            if !response.status().is_success() {
                consume_bounded(response, 64 * 1024).await?;
                return Err(Error::Network);
            }
            let bytes = consume_bounded(response, MAX_API_RESPONSE_BYTES).await?;
            let data = if bytes.is_empty() {
                None
            } else {
                Some(serde_json::from_slice(&bytes).map_err(|_| Error::InvalidGitHubResponse)?)
            };
            if status == 200
                && let (Some(key), Some(tag)) = (cache_key.clone(), etag.clone())
            {
                self.cache.lock().await.insert(key, tag, bytes);
            }
            return Ok(ApiResponse { status, etag, data });
        }
        Err(Error::Network)
    }

    async fn reserve_request(&self, method: &Method) -> Result<()> {
        let delay = {
            let mut budget = self.budget.lock().await;
            let now = Instant::now();
            if budget.cooldown.is_some_and(|deadline| deadline > now) {
                return Err(Error::RateLimited);
            }
            let mut next = budget.next_request.unwrap_or(now).max(now);
            if method != Method::GET {
                next = next.max(budget.next_write.unwrap_or(now));
            }
            budget.next_request = Some(next + Duration::from_millis(100));
            if method != Method::GET {
                budget.next_write = Some(next + Duration::from_secs(1));
            }
            next.saturating_duration_since(now)
        };
        sleep(delay).await;
        if self
            .budget
            .lock()
            .await
            .cooldown
            .is_some_and(|deadline| deadline > Instant::now())
        {
            return Err(Error::RateLimited);
        }
        Ok(())
    }

    pub async fn request_device_code(&self) -> Result<DeviceCode> {
        let value = self
            .post_login_form("/login/device/code", &[("client_id", CLIENT_ID)])
            .await?;
        parse_device_code(&value)
    }

    pub async fn poll_device_token(
        &self,
        device_code: &SecretString,
        repository_id: Option<u64>,
    ) -> Result<OAuthPoll> {
        let repository = repository_id.map(|value| value.to_string());
        let mut form = vec![
            ("client_id", CLIENT_ID),
            ("device_code", device_code.expose_secret()),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ];
        if let Some(value) = repository.as_deref() {
            form.push(("repository_id", value));
        }
        let value = self
            .post_login_form("/login/oauth/access_token", &form)
            .await?;
        parse_oauth_poll(&value)
    }

    pub async fn refresh_access_token(&self, refresh_token: &SecretString) -> Result<OAuthToken> {
        validate_token(refresh_token.expose_secret())?;
        let value = self
            .post_login_form(
                "/login/oauth/access_token",
                &[
                    ("client_id", CLIENT_ID),
                    ("grant_type", "refresh_token"),
                    ("refresh_token", refresh_token.expose_secret()),
                ],
            )
            .await?;
        match parse_oauth_poll(&value)? {
            OAuthPoll::Granted(token) => Ok(token),
            _ => Err(Error::SessionExpired),
        }
    }

    async fn post_login_form(&self, endpoint: &str, form: &[(&str, &str)]) -> Result<Value> {
        let url = fixed_origin_url(LOGIN_ORIGIN, endpoint)?;
        let response = self
            .client
            .post(url)
            .header(header::ACCEPT, "application/json")
            .form(form)
            .send()
            .await
            .map_err(|_| Error::Network)?;
        if !response.status().is_success() {
            consume_bounded(response, 64 * 1024).await?;
            return Err(Error::GitHubDenied);
        }
        let bytes = consume_bounded(response, 64 * 1024).await?;
        serde_json::from_slice(&bytes).map_err(|_| Error::InvalidGitHubResponse)
    }
}

pub fn normalize_viewer(value: &Value) -> Result<ViewerIdentity> {
    let object = value.as_object().ok_or(Error::InvalidGitHubResponse)?;
    let id = positive_u64(object, "id")?;
    let login = validation::github_login(required_string(object, "login", 39)?)?.to_owned();
    let avatar_url = optional_avatar_url(object.get("avatar_url"))?;
    let name = optional_string(object.get("name"), 256)?;
    Ok(ViewerIdentity {
        id,
        login,
        avatar_url,
        name,
    })
}

pub fn normalize_repository(
    value: &Value,
    expected_owner_id: u64,
    expected_name: &str,
) -> Result<RepositoryIdentity> {
    validation::repository_name_exact(expected_name)?;
    let object = value.as_object().ok_or(Error::InvalidGitHubResponse)?;
    let owner = object
        .get("owner")
        .and_then(Value::as_object)
        .ok_or(Error::InvalidGitHubResponse)?;
    if positive_u64(owner, "id")? != expected_owner_id
        || required_string(object, "name", 100)? != expected_name
        || object.get("private").and_then(Value::as_bool) != Some(false)
    {
        return Err(Error::InvalidGitHubResponse);
    }
    let url = validate_github_web_url(required_string(object, "html_url", 512)?)?;
    Ok(RepositoryIdentity {
        id: positive_u64(object, "id")?,
        name: expected_name.to_owned(),
        url: url.to_string(),
        description: optional_string(object.get("description"), 256)?,
    })
}

pub fn repository_creation_url(login: &str, name: &str) -> Result<Url> {
    let login = validation::github_login(login)?;
    validation::repository_name_exact(name)?;
    let mut url = fixed_origin_url(LOGIN_ORIGIN, "/new")?;
    url.query_pairs_mut()
        .append_pair("owner", login)
        .append_pair("name", name)
        .append_pair("description", REPOSITORY_DESCRIPTION)
        .append_pair("visibility", "public");
    Ok(url)
}

pub fn installation_url(account_id: u64, repository_id: u64) -> Result<Url> {
    if account_id == 0 || repository_id == 0 {
        return Err(Error::InvalidData);
    }
    let mut url = fixed_origin_url(
        LOGIN_ORIGIN,
        &format!("/apps/{APP_SLUG}/installations/new/permissions"),
    )?;
    url.query_pairs_mut()
        .append_pair("suggested_target_id", &account_id.to_string())
        .append_pair("repository_ids[]", &repository_id.to_string());
    Ok(url)
}

pub fn validate_installation(
    installation: &Value,
    repositories: &Value,
    account_id: u64,
    repository_id: u64,
) -> Result<u64> {
    let installation = installation.as_object().ok_or(Error::InvalidInstallation)?;
    let account = installation
        .get("account")
        .and_then(Value::as_object)
        .ok_or(Error::InvalidInstallation)?;
    let permissions = installation
        .get("permissions")
        .and_then(Value::as_object)
        .ok_or(Error::InvalidInstallation)?;
    if required_string(installation, "app_slug", 100)? != APP_SLUG
        || positive_u64(account, "id")? != account_id
        || required_string(installation, "repository_selection", 32)? != "selected"
        || permissions.keys().any(|key| {
            !matches!(
                key.as_str(),
                "contents" | "metadata" | "repository_creation"
            )
        })
        || permissions
            .get("repository_creation")
            .is_some_and(|value| value.as_str() != Some("write"))
        || permissions.get("contents").and_then(Value::as_str) != Some("write")
        || permissions.get("metadata").and_then(Value::as_str) != Some("read")
    {
        return Err(Error::InvalidInstallation);
    }
    let repositories = repositories.as_object().ok_or(Error::InvalidInstallation)?;
    let entries = repositories
        .get("repositories")
        .and_then(Value::as_array)
        .ok_or(Error::InvalidInstallation)?;
    let count = positive_u64(repositories, "total_count")?;
    if !(1..=2).contains(&count)
        || entries.len() != count as usize
        || entries
            .iter()
            .filter(|entry| entry.get("id").and_then(Value::as_u64) == Some(repository_id))
            .count()
            != 1
    {
        return Err(Error::InvalidInstallation);
    }
    for entry in entries
        .iter()
        .filter(|entry| entry.get("id").and_then(Value::as_u64) != Some(repository_id))
    {
        if entry
            .get("id")
            .and_then(Value::as_u64)
            .is_none_or(|id| id == 0)
            || entry.get("name").and_then(Value::as_str)
                != Some(crate::noosnote::repository_name(account_id).as_str())
            || entry.get("private").and_then(Value::as_bool) != Some(true)
            || entry.pointer("/owner/id").and_then(Value::as_u64) != Some(account_id)
        {
            return Err(Error::InvalidInstallation);
        }
    }
    positive_u64(installation, "id").map_err(|_| Error::InvalidInstallation)
}

fn parse_device_code(value: &Value) -> Result<DeviceCode> {
    let object = exact_object(
        value,
        &[
            "device_code",
            "user_code",
            "verification_uri",
            "expires_in",
            "interval",
        ],
    )?;
    let secret = required_string(object, "device_code", MAX_TOKEN_BYTES)?;
    validate_token(secret)?;
    let user_code = required_string(object, "user_code", 32)?;
    if user_code.is_empty()
        || !user_code
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(Error::InvalidGitHubResponse);
    }
    let verification_uri = Url::parse(required_string(object, "verification_uri", 256)?)
        .map_err(|_| Error::InvalidGitHubResponse)?;
    if verification_uri.as_str() != "https://github.com/login/device" {
        return Err(Error::InvalidGitHubResponse);
    }
    let interval = bounded_seconds(object, "interval", 1, 60)?;
    let expires_in = bounded_seconds(object, "expires_in", 60, 1_800)?;
    Ok(DeviceCode {
        secret: SecretString::from(secret.to_owned()),
        user_code: user_code.to_owned(),
        verification_uri,
        interval,
        expires_in,
    })
}

fn parse_oauth_poll(value: &Value) -> Result<OAuthPoll> {
    let object = value.as_object().ok_or(Error::InvalidGitHubResponse)?;
    if let Some(access_token) = object.get("access_token") {
        ensure_only_keys(
            object,
            &[
                "access_token",
                "expires_in",
                "refresh_token",
                "refresh_token_expires_in",
                "scope",
                "token_type",
            ],
        )?;
        let access_token = access_token.as_str().ok_or(Error::InvalidGitHubResponse)?;
        validate_token(access_token)?;
        if required_string(object, "token_type", 16)? != "bearer" {
            return Err(Error::InvalidGitHubResponse);
        }
        let refresh_token = optional_string(object.get("refresh_token"), MAX_TOKEN_BYTES)?
            .map(|token| {
                validate_token(&token)?;
                Ok::<SecretString, Error>(SecretString::from(token))
            })
            .transpose()?;
        return Ok(OAuthPoll::Granted(OAuthToken {
            access_token: SecretString::from(access_token.to_owned()),
            refresh_token,
            expires_in: optional_seconds(object.get("expires_in"), 60, 31_536_000)?,
            refresh_expires_in: optional_seconds(
                object.get("refresh_token_expires_in"),
                60,
                63_072_000,
            )?,
        }));
    }

    ensure_only_keys(object, &["error", "error_description", "error_uri"])?;
    match required_string(object, "error", 64)? {
        "authorization_pending" => Ok(OAuthPoll::Pending),
        "slow_down" => Ok(OAuthPoll::SlowDown),
        "access_denied" => Ok(OAuthPoll::Denied),
        "expired_token" => Ok(OAuthPoll::Expired),
        _ => Err(Error::InvalidGitHubResponse),
    }
}

fn exact_object<'a>(value: &'a Value, keys: &[&str]) -> Result<&'a Map<String, Value>> {
    let object = value.as_object().ok_or(Error::InvalidGitHubResponse)?;
    ensure_only_keys(object, keys)?;
    if object.len() != keys.len() || keys.iter().any(|key| !object.contains_key(*key)) {
        return Err(Error::InvalidGitHubResponse);
    }
    Ok(object)
}

fn ensure_only_keys(object: &Map<String, Value>, keys: &[&str]) -> Result<()> {
    if object.keys().all(|key| keys.contains(&key.as_str())) {
        Ok(())
    } else {
        Err(Error::InvalidGitHubResponse)
    }
}

fn required_string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    maximum: usize,
) -> Result<&'a str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= maximum)
        .ok_or(Error::InvalidGitHubResponse)
}

fn optional_string(value: Option<&Value>, maximum: usize) -> Result<Option<String>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if value.len() <= maximum => Ok(Some(value.clone())),
        _ => Err(Error::InvalidGitHubResponse),
    }
}

fn positive_u64(object: &Map<String, Value>, key: &str) -> Result<u64> {
    object
        .get(key)
        .and_then(Value::as_u64)
        .filter(|value| *value > 0 && *value <= 9_007_199_254_740_991)
        .ok_or(Error::InvalidGitHubResponse)
}

fn bounded_seconds(
    object: &Map<String, Value>,
    key: &str,
    minimum: u64,
    maximum: u64,
) -> Result<Duration> {
    optional_seconds(object.get(key), minimum, maximum)?.ok_or(Error::InvalidGitHubResponse)
}

fn optional_seconds(value: Option<&Value>, minimum: u64, maximum: u64) -> Result<Option<Duration>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .filter(|seconds| (minimum..=maximum).contains(seconds))
            .map(Duration::from_secs)
            .map(Some)
            .ok_or(Error::InvalidGitHubResponse),
    }
}

fn validate_token(value: &str) -> Result<()> {
    if !value.is_empty()
        && value.len() <= MAX_TOKEN_BYTES
        && value.bytes().all(|byte| byte.is_ascii_graphic())
    {
        Ok(())
    } else {
        Err(Error::InvalidGitHubResponse)
    }
}

fn optional_avatar_url(value: Option<&Value>) -> Result<String> {
    let Some(value) = optional_string(value, 512)? else {
        return Ok(String::new());
    };
    let url = Url::parse(&value).map_err(|_| Error::InvalidGitHubResponse)?;
    if url.scheme() != "https"
        || url.host_str() != Some("avatars.githubusercontent.com")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(Error::InvalidGitHubResponse);
    }
    Ok(url.to_string())
}

fn validate_github_web_url(value: &str) -> Result<Url> {
    let url = Url::parse(value).map_err(|_| Error::InvalidGitHubResponse)?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::InvalidGitHubResponse);
    }
    Ok(url)
}

fn rate_limit_delay(status: u16, headers: &header::HeaderMap, now: SystemTime) -> Option<Duration> {
    let seconds = |name| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
    };
    if status != 429
        && !(status == 403
            && (seconds("x-ratelimit-remaining") == Some(0) || headers.contains_key("retry-after")))
    {
        return None;
    }
    let retry = seconds("retry-after").or_else(|| {
        let value = headers.get("retry-after")?.to_str().ok()?;
        let deadline =
            time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc2822)
                .ok()?
                .unix_timestamp();
        let current = now.duration_since(UNIX_EPOCH).ok()?.as_secs();
        Some(u64::try_from(deadline).ok()?.saturating_sub(current))
    });
    let reset = seconds("x-ratelimit-reset").map(|reset| {
        reset
            .saturating_sub(now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs())
            .saturating_add(1)
    });
    Some(Duration::from_secs(
        retry.or(reset).unwrap_or(60).clamp(1, 86_400),
    ))
}

pub fn retry_delay(attempt: u8) -> Duration {
    let exponent = u32::from(attempt.min(6));
    let ceiling_ms = 500_u64.saturating_mul(2_u64.pow(exponent));
    let jitter_ms = rand::rng().random_range(0..=ceiling_ms / 2);
    Duration::from_millis((ceiling_ms / 2 + jitter_ms).min(30_000))
}

fn fixed_origin_url(origin: &str, endpoint: &str) -> Result<Url> {
    if !endpoint.starts_with('/')
        || endpoint.starts_with("//")
        || endpoint.contains('\\')
        || endpoint.contains('\r')
        || endpoint.contains('\n')
        || endpoint
            .split('/')
            .any(|segment| segment == ".." || segment == ".")
    {
        return Err(Error::InvalidData);
    }
    let url = Url::parse(&format!("{origin}{endpoint}")).map_err(|_| Error::InvalidData)?;
    let expected = Url::parse(origin).map_err(|_| Error::InvalidData)?;
    if url.scheme() != "https"
        || url.host_str() != expected.host_str()
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(Error::InvalidData);
    }
    Ok(url)
}

async fn consume_bounded(response: Response, maximum: usize) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > maximum as u64)
    {
        return Err(Error::ResponseTooLarge);
    }
    let bytes = response.bytes().await.map_err(|_| Error::Network)?;
    if bytes.len() > maximum {
        return Err(Error::ResponseTooLarge);
    }
    Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn http_fixture(responses: Vec<&'static str>) -> (Url, std::thread::JoinHandle<Vec<String>>) {
        use std::io::{Read as _, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = Url::parse(&format!(
            "http://{}/fixture",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        let worker = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                while !bytes.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    bytes.push(byte[0]);
                    assert!(bytes.len() < 16 * 1024);
                }
                requests.push(String::from_utf8(bytes).unwrap().to_ascii_lowercase());
                stream.write_all(response.as_bytes()).unwrap();
            }
            requests
        });
        (url, worker)
    }

    const CACHED_OK: &str = "HTTP/1.1 200 OK\r\nETag: \"first\"\r\nContent-Length: 16\r\nConnection: close\r\n\r\n{\"generation\":1}";
    const CACHED_CHANGED: &str = "HTTP/1.1 200 OK\r\nETag: \"second\"\r\nContent-Length: 16\r\nConnection: close\r\n\r\n{\"generation\":2}";
    const NOT_MODIFIED: &str =
        "HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

    #[tokio::test]
    async fn unchanged_authenticated_reads_reuse_json_and_refresh_when_changed() {
        let (url, server) = http_fixture(vec![CACHED_OK, NOT_MODIFIED, CACHED_CHANGED]);
        let client = GitHubClient::new().unwrap();
        for expected in [1, 1, 2] {
            let response: ApiResponse<Value> = client
                .send_api_json(
                    Method::GET,
                    url.clone(),
                    Some("fixture-token"),
                    None,
                    None,
                    &[],
                    "application/json",
                )
                .await
                .unwrap();
            assert_eq!(response.status, 200);
            assert_eq!(response.data.unwrap()["generation"], expected);
        }
        let requests = server.join().unwrap();
        assert!(!requests[0].contains("if-none-match"));
        assert!(requests[1].contains("if-none-match: \"first\""));
        assert!(requests[2].contains("if-none-match: \"first\""));
        assert_eq!(client.budget.lock().await.unchanged, 1);
    }

    #[tokio::test]
    async fn conditional_cache_is_bound_to_credentials_and_representation() {
        let (url, server) = http_fixture(vec![CACHED_OK, CACHED_OK, CACHED_OK, NOT_MODIFIED]);
        let client = GitHubClient::new().unwrap();
        for (token, accept, explicit) in [
            ("first-token", "application/json", None),
            ("second-token", "application/json", None),
            ("first-token", "application/vnd.github.star+json", None),
            ("first-token", "application/json", Some("\"explicit\"")),
        ] {
            let response: ApiResponse<Value> = client
                .send_api_json(
                    Method::GET,
                    url.clone(),
                    Some(token),
                    explicit,
                    None,
                    &[],
                    accept,
                )
                .await
                .unwrap();
            if explicit.is_some() {
                assert_eq!(response.status, 304);
                assert!(response.data.is_none());
            } else {
                assert_eq!(response.data.unwrap()["generation"], 1);
            }
        }
        let requests = server.join().unwrap();
        assert!(
            requests[..3]
                .iter()
                .all(|request| !request.contains("if-none-match"))
        );
        assert!(requests[3].contains("if-none-match: \"explicit\""));
    }

    #[tokio::test]
    async fn unauthenticated_requests_do_not_reuse_an_authenticated_response() {
        let (url, server) = http_fixture(vec![CACHED_OK, CACHED_CHANGED]);
        let client = GitHubClient::new().unwrap();
        for token in [Some("fixture-token"), None] {
            let _: ApiResponse<Value> = client
                .send_api_json(
                    Method::GET,
                    url.clone(),
                    token,
                    None,
                    None,
                    &[],
                    "application/json",
                )
                .await
                .unwrap();
        }
        assert!(
            server
                .join()
                .unwrap()
                .iter()
                .all(|request| !request.contains("if-none-match"))
        );
    }

    #[test]
    fn conditional_cache_bounds_response_count_and_total_memory() {
        let mut cache = ResponseCache::default();
        for index in 0..65 {
            cache.insert(
                format!("request-{index:02}"),
                "etag".to_owned(),
                vec![0; 1024 * 1024],
            );
        }
        assert_eq!(cache.entries.len(), 16);
        assert_eq!(cache.size, 16 * 1024 * 1024);
        assert!(cache.get("request-00").is_none());
        assert!(cache.get("request-64").is_some());
        cache.insert(
            "oversized".to_owned(),
            "etag".to_owned(),
            vec![0; 1024 * 1024 + 1],
        );
        assert!(cache.get("oversized").is_none());
    }

    #[test]
    fn rate_limits_use_server_deadlines_without_retrying_denials() {
        let mut headers = header::HeaderMap::new();
        headers.insert("retry-after", header::HeaderValue::from_static("120"));
        assert_eq!(
            rate_limit_delay(429, &headers, UNIX_EPOCH),
            Some(Duration::from_secs(120))
        );
        headers.remove("retry-after");
        headers.insert(
            "x-ratelimit-remaining",
            header::HeaderValue::from_static("0"),
        );
        headers.insert(
            "x-ratelimit-reset",
            header::HeaderValue::from_static("3600"),
        );
        assert_eq!(
            rate_limit_delay(403, &headers, UNIX_EPOCH + Duration::from_secs(100)),
            Some(Duration::from_secs(3501))
        );
        headers.clear();
        assert_eq!(rate_limit_delay(403, &headers, UNIX_EPOCH), None);
        assert_eq!(
            rate_limit_delay(429, &headers, UNIX_EPOCH),
            Some(Duration::from_secs(60))
        );
    }

    #[tokio::test]
    async fn clones_share_the_rate_limit_cooldown() {
        let client = GitHubClient::new().unwrap();
        client.budget.lock().await.cooldown = Some(Instant::now() + Duration::from_secs(60));
        assert!(matches!(
            client.clone().reserve_request(&Method::GET).await,
            Err(Error::RateLimited)
        ));
    }

    #[test]
    fn api_urls_cannot_escape_the_github_origin() {
        assert!(fixed_origin_url(API_ORIGIN, "/user").is_ok());
        for hostile in [
            "https://attacker.invalid/",
            "//attacker.invalid/",
            "/../login",
            "/repos\\..\\token",
            "/user\r\nX-Test: injected",
        ] {
            assert!(fixed_origin_url(API_ORIGIN, hostile).is_err(), "{hostile}");
        }
    }

    #[test]
    fn backoff_is_bounded_and_jittered() {
        for attempt in 0..=20 {
            assert!(retry_delay(attempt) <= Duration::from_secs(30));
        }
    }

    #[test]
    fn device_flow_rejects_unknown_fields_and_host_confusion() {
        let valid = serde_json::json!({
            "device_code": "device-secret",
            "user_code": "ABCD-1234",
            "verification_uri": "https://github.com/login/device",
            "expires_in": 900,
            "interval": 5
        });
        assert!(parse_device_code(&valid).is_ok());

        let mut unknown = valid.clone();
        unknown["redirect"] = Value::String("https://attacker.invalid".to_owned());
        assert!(parse_device_code(&unknown).is_err());

        let mut hostile = valid;
        hostile["verification_uri"] =
            Value::String("https://github.com.attacker.invalid/login/device".to_owned());
        assert!(parse_device_code(&hostile).is_err());
    }

    #[test]
    fn oauth_tokens_are_bounded_and_strict() {
        let valid = serde_json::json!({
            "access_token": "ghu_example",
            "expires_in": 28_800,
            "refresh_token": "ghr_example",
            "refresh_token_expires_in": 15_897_600,
            "scope": "",
            "token_type": "bearer"
        });
        assert!(matches!(
            parse_oauth_poll(&valid).unwrap(),
            OAuthPoll::Granted(_)
        ));

        let mut injected = valid.clone();
        injected["access_token"] = Value::String("token\r\nHeader: value".to_owned());
        assert!(parse_oauth_poll(&injected).is_err());

        let mut unknown = valid;
        unknown["unexpected"] = Value::Bool(true);
        assert!(parse_oauth_poll(&unknown).is_err());
    }

    #[test]
    fn github_identities_reject_wrong_owners_and_urls() {
        let viewer = serde_json::json!({
            "id": 42,
            "login": "octocat",
            "avatar_url": "https://avatars.githubusercontent.com/u/42?v=4",
            "name": "The Octocat",
            "ignored_forward_compatible_field": true
        });
        assert_eq!(normalize_viewer(&viewer).unwrap().id, 42);

        let repository = serde_json::json!({
            "id": 420,
            "name": "noosphere_user_octocat",
            "html_url": "https://github.com/octocat/noosphere_user_octocat",
            "private": false,
            "owner": { "id": 42 },
            "description": REPOSITORY_DESCRIPTION
        });
        assert!(normalize_repository(&repository, 42, "noosphere_user_octocat").is_ok());
        assert!(normalize_repository(&repository, 99, "noosphere_user_octocat").is_err());
        let mut hostile = repository;
        hostile["html_url"] = Value::String("https://github.com.attacker.invalid/repo".to_owned());
        assert!(normalize_repository(&hostile, 42, "noosphere_user_octocat").is_err());
    }

    #[test]
    fn installation_is_limited_to_dedicated_repositories_and_exact_permissions() {
        let installation = serde_json::json!({
            "id": 7,
            "app_slug": APP_SLUG,
            "account": { "id": 42 },
            "repository_selection": "selected",
            "permissions": { "contents": "write", "metadata": "read" }
        });
        let repositories = serde_json::json!({
            "total_count": 1,
            "repositories": [{ "id": 420 }]
        });
        assert_eq!(
            validate_installation(&installation, &repositories, 42, 420).unwrap(),
            7
        );

        let notes =
            serde_json::json!({"id":421,"name":"noosnote_42","private":true,"owner":{"id":42}});
        let dedicated = serde_json::json!({"total_count":2,"repositories":[{"id":420},notes]});
        assert_eq!(
            validate_installation(&installation, &dedicated, 42, 420).unwrap(),
            7
        );
        let mut public = dedicated.clone();
        public["repositories"][1]["private"] = serde_json::json!(false);
        assert!(validate_installation(&installation, &public, 42, 420).is_err());
        let mut foreign = dedicated.clone();
        foreign["repositories"][1]["owner"]["id"] = serde_json::json!(99);
        assert!(validate_installation(&installation, &foreign, 42, 420).is_err());
        let mut unrelated = dedicated;
        unrelated["repositories"][1]["name"] = serde_json::json!("my-project");
        assert!(validate_installation(&installation, &unrelated, 42, 420).is_err());

        let mut broad = installation.clone();
        broad["repository_selection"] = Value::String("all".to_owned());
        assert!(validate_installation(&broad, &repositories, 42, 420).is_err());

        let mut excessive = installation;
        excessive["permissions"]["issues"] = Value::String("write".to_owned());
        assert!(validate_installation(&excessive, &repositories, 42, 420).is_err());
    }

    #[test]
    fn generated_browser_urls_keep_the_github_origin() {
        let repository = repository_creation_url("octocat", "noosphere_user_octocat").unwrap();
        assert_eq!(repository.host_str(), Some("github.com"));
        assert!(repository.as_str().contains("visibility=public"));

        let installation = installation_url(42, 420).unwrap();
        assert_eq!(installation.host_str(), Some("github.com"));
        assert!(installation.as_str().contains("repository_ids%5B%5D=420"));
    }
}
