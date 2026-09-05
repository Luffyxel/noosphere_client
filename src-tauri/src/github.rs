use std::time::Duration;

use rand::Rng as _;
use reqwest::{Client, Method, Response, header};
use secrecy::{ExposeSecret as _, SecretString};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use tokio::time::sleep;
use url::Url;

use crate::{
    error::{Error, Result},
    validation,
};

const API_ORIGIN: &str = "https://api.github.com";
const LOGIN_ORIGIN: &str = "https://github.com";
const MAX_API_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
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
}

impl GitHubClient {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("Noosphere/0.1")
            .build()
            .map_err(|_| Error::Network)?;
        Ok(Self { client })
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
        let url = fixed_origin_url(API_ORIGIN, endpoint)?;
        if let Some(token) = access_token {
            validate_token(token)?;
        }
        for attempt in 0..=3 {
            let mut request = self
                .client
                .request(method.clone(), url.clone())
                .header(header::ACCEPT, "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28");
            if let Some(token) = access_token {
                request = request.bearer_auth(token);
            }
            if let Some(value) = etag {
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
            return Ok(ApiResponse { status, etag, data });
        }
        Err(Error::Network)
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
        || permissions.len() != 2
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
    if positive_u64(repositories, "total_count")? != 1
        || entries.len() != 1
        || entries[0].get("id").and_then(Value::as_u64) != Some(repository_id)
    {
        return Err(Error::InvalidInstallation);
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
    fn installation_is_limited_to_one_repository_and_exact_permissions() {
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
