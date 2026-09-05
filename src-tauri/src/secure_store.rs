use secrecy::{ExposeSecret as _, SecretString};

use crate::error::{Error, Result};

const SERVICE: &str = "org.noosphere.desktop";

pub trait SecretStore: Send + Sync {
    fn load(&self, account: &str) -> Result<Option<SecretString>>;
    fn save(&self, account: &str, value: SecretString) -> Result<()>;
    fn remove(&self, account: &str) -> Result<()>;
}

#[derive(Default)]
pub struct SystemSecretStore;

impl SystemSecretStore {
    fn entry(account: &str) -> Result<keyring::Entry> {
        if account.is_empty() || account.len() > 128 || account.chars().any(char::is_control) {
            return Err(Error::InvalidData);
        }
        keyring::Entry::new(SERVICE, account).map_err(|_| Error::SecureStorageUnavailable)
    }
}

impl SecretStore for SystemSecretStore {
    fn load(&self, account: &str) -> Result<Option<SecretString>> {
        match Self::entry(account)?.get_password() {
            Ok(value) => Ok(Some(SecretString::from(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(Error::SecureStorageUnavailable),
        }
    }

    fn save(&self, account: &str, value: SecretString) -> Result<()> {
        Self::entry(account)?
            .set_password(value.expose_secret())
            .map_err(|_| Error::SecureStorageUnavailable)
    }

    fn remove(&self, account: &str) -> Result<()> {
        match Self::entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(Error::SecureStorageUnavailable),
        }
    }
}
