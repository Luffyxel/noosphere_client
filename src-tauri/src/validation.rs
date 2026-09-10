use crate::error::{Error, Result};

pub const MAX_MESSAGE_BYTES: usize = 16 * 1024;

pub fn github_login(value: &str) -> Result<&str> {
    let value = value.trim().strip_prefix('@').unwrap_or(value.trim());
    let valid_length = !value.is_empty() && value.len() <= 39;
    let valid_edges = value
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric);
    let valid_characters = value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
    if valid_length && valid_edges && valid_characters && !value.contains("--") {
        Ok(value)
    } else {
        Err(Error::InvalidData)
    }
}

pub fn conversation_id(value: &str) -> Result<&str> {
    opaque_id(value, "dm-")
}

pub fn message_id(value: &str) -> Result<&str> {
    opaque_id(value, "msg-")
}

pub fn realtime_session_id(value: &str) -> Result<&str> {
    opaque_id(value, "rtc-")
}

pub fn call_id(value: &str) -> Result<&str> {
    opaque_id(value, "call-")
}

fn opaque_id<'a>(value: &'a str, prefix: &str) -> Result<&'a str> {
    let suffix = value.strip_prefix(prefix).ok_or(Error::InvalidData)?;
    if suffix.len() == 32
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(value)
    } else {
        Err(Error::InvalidData)
    }
}

pub fn message_text(value: &str) -> Result<&str> {
    if !value.trim().is_empty()
        && value.chars().count() <= 4_000
        && value.len() <= MAX_MESSAGE_BYTES
    {
        Ok(value)
    } else {
        Err(Error::InvalidData)
    }
}

pub fn notification_text(value: &str) -> Result<&str> {
    if value.len() <= 256 && !value.chars().any(char::is_control) {
        Ok(value)
    } else {
        Err(Error::InvalidData)
    }
}

pub fn repository_name(login: &str, github_user_id: u64, attempt: u8) -> Result<String> {
    let login = github_login(login)?.to_ascii_lowercase();
    if github_user_id == 0 || attempt > 32 {
        return Err(Error::InvalidData);
    }
    let base = format!("noosphere_user_{login}");
    Ok(match attempt {
        0 => base,
        1 => format!("{base}_{github_user_id}"),
        _ => format!("{base}_{github_user_id}_{attempt}"),
    })
}

pub fn repository_name_exact(value: &str) -> Result<&str> {
    if !value.is_empty()
        && value.len() <= 100
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        Ok(value)
    } else {
        Err(Error::InvalidData)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_traversal_and_malformed_ids() {
        assert!(conversation_id("dm-0123456789abcdef0123456789abcdef").is_ok());
        assert!(conversation_id("../dm-0123456789abcdef0123456789abcdef").is_err());
        assert!(message_id("msg-0123456789abcdef0123456789abcdef").is_ok());
        assert!(message_id("msg-0123456789abcdef0123456789abcdeg").is_err());
        assert!(realtime_session_id("rtc-0123456789abcdef0123456789abcdef").is_ok());
        assert!(realtime_session_id("rtc-0123456789abcdef0123456789abcdeg").is_err());
        assert!(call_id("call-0123456789abcdef0123456789abcdef").is_ok());
        assert!(call_id("call-0123456789abcdef0123456789abcdeg").is_err());
    }

    #[test]
    fn repository_collision_suffix_is_deterministic() {
        assert_eq!(
            repository_name("Octo-Cat", 42, 0).unwrap(),
            "noosphere_user_octo-cat"
        );
        assert_eq!(
            repository_name("Octo-Cat", 42, 1).unwrap(),
            "noosphere_user_octo-cat_42"
        );
        assert_eq!(
            repository_name("Octo-Cat", 42, 2).unwrap(),
            "noosphere_user_octo-cat_42_2"
        );
        assert!(repository_name_exact("noosphere_user_octocat").is_ok());
        assert!(repository_name_exact("../secrets").is_err());
    }

    #[test]
    fn message_size_is_bounded_in_bytes() {
        assert!(message_text("bonjour").is_ok());
        assert!(message_text("   ").is_err());
        assert!(message_text(&"é".repeat(8_193)).is_err());
    }
}
