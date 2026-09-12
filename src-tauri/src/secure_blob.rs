use std::path::{Path, PathBuf};

#[cfg(windows)]
use secrecy::ExposeSecret as _;
use secrecy::SecretString;
#[cfg(target_os = "linux")]
use zeroize::Zeroizing;

use crate::error::{Error, Result};

#[cfg(windows)]
const MAX_PROTECTED_BYTES: usize = 32 * 1024 * 1024;
#[cfg(target_os = "linux")]
const MAX_PROTECTED_BYTES: usize = 32 * 1024 * 1024 + 32;

#[derive(Clone)]
pub struct SecureBlobStore {
    #[cfg(windows)]
    directory: PathBuf,
    #[cfg(target_os = "linux")]
    directory: PathBuf,
    #[cfg(target_os = "linux")]
    key: Zeroizing<[u8; 32]>,
}

impl SecureBlobStore {
    pub fn new(profile_directory: &Path) -> Result<Self> {
        let directory = profile_directory.join("protected");
        std::fs::create_dir_all(&directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
        }
        #[cfg(windows)]
        return Ok(Self { directory });
        #[cfg(target_os = "linux")]
        {
            let key = linux::load_or_create_key(&directory)?;
            Ok(Self {
                directory,
                key: Zeroizing::new(key),
            })
        }
    }

    pub fn load(&self, account: &str) -> Result<Option<SecretString>> {
        validate_account(account)?;
        #[cfg(windows)]
        {
            let path = self.path(account);
            let protected = match std::fs::read(path) {
                Ok(value) => value,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(_) => return Err(Error::SecureStorageUnavailable),
            };
            if protected.is_empty() || protected.len() > MAX_PROTECTED_BYTES {
                return Err(Error::SecureStorageUnavailable);
            }
            windows::unprotect(account, &protected).map(Some)
        }
        #[cfg(target_os = "linux")]
        {
            let path = self.path(account);
            let protected = match std::fs::read(&path) {
                Ok(value) => value,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    if self.tombstone_path(account).exists() {
                        return Ok(None);
                    }
                    let Some(value) = linux::load_unlocked_legacy_secret(account) else {
                        return Ok(None);
                    };
                    self.save(account, value.clone())?;
                    return Ok(Some(value));
                }
                Err(_) => return Err(Error::SecureStorageUnavailable),
            };
            if protected.len() < linux::HEADER_BYTES + linux::TAG_BYTES
                || protected.len() > MAX_PROTECTED_BYTES
            {
                return Err(Error::SecureStorageUnavailable);
            }
            linux::unprotect(account, &self.key, &protected).map(Some)
        }
    }

    pub fn save(&self, account: &str, value: SecretString) -> Result<()> {
        validate_account(account)?;
        #[cfg(windows)]
        {
            let protected = windows::protect(account, value.expose_secret().as_bytes())?;
            if protected.is_empty() || protected.len() > MAX_PROTECTED_BYTES {
                return Err(Error::SecureStorageUnavailable);
            }
            windows::atomic_write(&self.path(account), &protected)
        }
        #[cfg(target_os = "linux")]
        {
            let protected = linux::protect(account, &self.key, value)?;
            if protected.len() > MAX_PROTECTED_BYTES {
                return Err(Error::SecureStorageUnavailable);
            }
            linux::atomic_write(&self.path(account), &protected)?;
            match std::fs::remove_file(self.tombstone_path(account)) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(_) => Err(Error::SecureStorageUnavailable),
            }
        }
    }

    #[cfg(any(target_os = "linux", all(test, windows)))]
    pub fn remove(&self, account: &str) -> Result<()> {
        validate_account(account)?;
        #[cfg(windows)]
        {
            match std::fs::remove_file(self.path(account)) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(_) => Err(Error::SecureStorageUnavailable),
            }
        }
        #[cfg(target_os = "linux")]
        {
            match std::fs::remove_file(self.path(account)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(Error::SecureStorageUnavailable),
            }
            linux::atomic_write(&self.tombstone_path(account), b"signed-out")
        }
    }

    fn path(&self, account: &str) -> PathBuf {
        self.directory.join(format!("{account}.bin"))
    }

    #[cfg(target_os = "linux")]
    fn tombstone_path(&self, account: &str) -> PathBuf {
        self.directory.join(format!("{account}.deleted"))
    }
}

fn validate_account(account: &str) -> Result<()> {
    if !account.is_empty()
        && account.len() <= 64
        && account
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        Ok(())
    } else {
        Err(Error::InvalidData)
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::{
        collections::HashMap,
        fs::OpenOptions,
        io::Write as _,
        os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _},
        path::Path,
    };

    use aes_gcm_siv::{
        Aes256GcmSiv,
        aead::{Aead as _, KeyInit as _, Payload, array::Array},
    };
    use rand::{TryRngCore as _, rngs::OsRng};
    use secrecy::{ExposeSecret as _, SecretString};
    use secret_service::{EncryptionType, blocking::SecretService};
    use uuid::Uuid;
    use zeroize::Zeroize as _;

    use crate::error::{Error, Result};

    const MAGIC: &[u8; 4] = b"NSL1";
    const NONCE_BYTES: usize = 12;
    pub const HEADER_BYTES: usize = MAGIC.len() + NONCE_BYTES;
    pub const TAG_BYTES: usize = 16;

    pub fn load_or_create_key(directory: &Path) -> Result<[u8; 32]> {
        let path = directory.join("linux-store.key");
        match std::fs::read(&path) {
            Ok(bytes) => {
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                    .map_err(|_| Error::SecureStorageUnavailable)?;
                return bytes
                    .try_into()
                    .map_err(|_| Error::SecureStorageUnavailable);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(Error::SecureStorageUnavailable),
        }
        let mut key = [0_u8; 32];
        OsRng
            .try_fill_bytes(&mut key)
            .map_err(|_| Error::SecureStorageUnavailable)?;
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                key.zeroize();
                let bytes = std::fs::read(path).map_err(|_| Error::SecureStorageUnavailable)?;
                return bytes
                    .try_into()
                    .map_err(|_| Error::SecureStorageUnavailable);
            }
            Err(_) => {
                key.zeroize();
                return Err(Error::SecureStorageUnavailable);
            }
        };
        if file.write_all(&key).is_err() || file.sync_all().is_err() {
            drop(file);
            let _ = std::fs::remove_file(&path);
            key.zeroize();
            return Err(Error::SecureStorageUnavailable);
        }
        Ok(key)
    }

    pub fn protect(account: &str, key: &[u8; 32], value: SecretString) -> Result<Vec<u8>> {
        let mut nonce = [0_u8; NONCE_BYTES];
        OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|_| Error::SecureStorageUnavailable)?;
        let cipher = Aes256GcmSiv::new(&Array(*key));
        let ciphertext = cipher
            .encrypt(
                &Array(nonce),
                Payload {
                    msg: value.expose_secret().as_bytes(),
                    aad: account.as_bytes(),
                },
            )
            .map_err(|_| Error::SecureStorageUnavailable)?;
        let mut protected = Vec::with_capacity(HEADER_BYTES + ciphertext.len());
        protected.extend_from_slice(MAGIC);
        protected.extend_from_slice(&nonce);
        protected.extend_from_slice(&ciphertext);
        Ok(protected)
    }

    pub fn unprotect(account: &str, key: &[u8; 32], protected: &[u8]) -> Result<SecretString> {
        if protected.get(..MAGIC.len()) != Some(MAGIC) {
            return Err(Error::SecureStorageUnavailable);
        }
        let nonce: [u8; NONCE_BYTES] = protected[MAGIC.len()..HEADER_BYTES]
            .try_into()
            .map_err(|_| Error::SecureStorageUnavailable)?;
        let cipher = Aes256GcmSiv::new(&Array(*key));
        let plaintext = cipher
            .decrypt(
                &Array(nonce),
                Payload {
                    msg: &protected[HEADER_BYTES..],
                    aad: account.as_bytes(),
                },
            )
            .map_err(|_| Error::SecureStorageUnavailable)?;
        String::from_utf8(plaintext)
            .map(SecretString::from)
            .map_err(|_| Error::SecureStorageUnavailable)
    }

    pub fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
        let parent = path.parent().ok_or(Error::SecureStorageUnavailable)?;
        let temporary = parent.join(format!(".{}.tmp", Uuid::new_v4()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|_| Error::SecureStorageUnavailable)?;
        if file.write_all(content).is_err() || file.sync_all().is_err() {
            let _ = std::fs::remove_file(&temporary);
            return Err(Error::SecureStorageUnavailable);
        }
        drop(file);
        if std::fs::rename(&temporary, path).is_err() {
            let _ = std::fs::remove_file(&temporary);
            return Err(Error::SecureStorageUnavailable);
        }
        Ok(())
    }

    pub fn load_unlocked_legacy_secret(account: &str) -> Option<SecretString> {
        let connection = zbus::blocking::Connection::session().ok()?;
        let dbus = zbus::blocking::fdo::DBusProxy::new(&connection).ok()?;
        let secret_service_name = "org.freedesktop.secrets".try_into().ok()?;
        if !dbus.name_has_owner(secret_service_name).ok()? {
            return None;
        }
        drop(dbus);
        let service = SecretService::connect_with_existing(EncryptionType::Dh, connection).ok()?;
        let search = service
            .search_items(HashMap::from([
                ("service", "org.noosphere.desktop"),
                ("username", account),
            ]))
            .ok()?;
        let item = search.unlocked.first()?;
        let bytes = item.get_secret().ok()?;
        String::from_utf8(bytes).ok().map(SecretString::from)
    }
}

#[cfg(windows)]
mod windows {
    use std::{
        ffi::OsStr, fs::OpenOptions, io::Write as _, os::windows::ffi::OsStrExt as _, path::Path,
        ptr, slice,
    };

    use secrecy::SecretString;
    use uuid::Uuid;
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
        Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW},
    };

    use crate::error::{Error, Result};

    pub fn protect(account: &str, plaintext: &[u8]) -> Result<Vec<u8>> {
        crypt(account, plaintext, true).map(Protected::into_vec)
    }

    pub fn unprotect(account: &str, protected: &[u8]) -> Result<SecretString> {
        let plaintext = crypt(account, protected, false)?;
        let text =
            String::from_utf8(plaintext.into_vec()).map_err(|_| Error::SecureStorageUnavailable)?;
        Ok(SecretString::from(text))
    }

    fn crypt(account: &str, input: &[u8], protect: bool) -> Result<Protected> {
        let input_len = u32::try_from(input.len()).map_err(|_| Error::SecureStorageUnavailable)?;
        let input = CRYPT_INTEGER_BLOB {
            cbData: input_len,
            pbData: input.as_ptr().cast_mut(),
        };
        let entropy_bytes = format!("org.noosphere.desktop/{account}").into_bytes();
        let entropy_len =
            u32::try_from(entropy_bytes.len()).map_err(|_| Error::SecureStorageUnavailable)?;
        let entropy = CRYPT_INTEGER_BLOB {
            cbData: entropy_len,
            pbData: entropy_bytes.as_ptr().cast_mut(),
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        // SAFETY: all input slices remain alive for the call, output is initialized by
        // DPAPI, and Protected owns and releases that allocation with LocalFree.
        let succeeded = unsafe {
            if protect {
                CryptProtectData(
                    &input,
                    ptr::null(),
                    &entropy,
                    ptr::null(),
                    ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
            } else {
                CryptUnprotectData(
                    &input,
                    ptr::null_mut(),
                    &entropy,
                    ptr::null(),
                    ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
            }
        };
        if succeeded == 0 || output.pbData.is_null() || output.cbData == 0 {
            return Err(Error::SecureStorageUnavailable);
        }
        Ok(Protected {
            pointer: output.pbData,
            length: output.cbData as usize,
            sensitive: !protect,
        })
    }

    struct Protected {
        pointer: *mut u8,
        length: usize,
        sensitive: bool,
    }

    impl Protected {
        fn into_vec(mut self) -> Vec<u8> {
            // SAFETY: DPAPI returned a valid allocation of `length` bytes.
            let value = unsafe { slice::from_raw_parts(self.pointer, self.length) }.to_vec();
            self.release();
            value
        }

        fn release(&mut self) {
            if self.pointer.is_null() {
                return;
            }
            // SAFETY: the allocation belongs to this value and is released once.
            unsafe {
                if self.sensitive {
                    ptr::write_bytes(self.pointer, 0, self.length);
                }
                LocalFree(self.pointer.cast());
            }
            self.pointer = ptr::null_mut();
            self.length = 0;
        }
    }

    impl Drop for Protected {
        fn drop(&mut self) {
            self.release();
        }
    }

    pub fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
        let parent = path.parent().ok_or(Error::SecureStorageUnavailable)?;
        let temporary = parent.join(format!(".{}.tmp", Uuid::new_v4()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| Error::SecureStorageUnavailable)?;
        if file.write_all(content).is_err() || file.sync_all().is_err() {
            let _ = std::fs::remove_file(&temporary);
            return Err(Error::SecureStorageUnavailable);
        }
        drop(file);
        let from = wide(&temporary);
        let to = wide(path);
        // SAFETY: both paths are NUL-terminated UTF-16 buffers valid for the call.
        let moved = unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if moved == 0 {
            let _ = std::fs::remove_file(&temporary);
            return Err(Error::SecureStorageUnavailable);
        }
        Ok(())
    }

    fn wide(path: &Path) -> Vec<u16> {
        OsStr::new(path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }
}

#[cfg(all(test, windows))]
mod tests {
    use secrecy::SecretString;

    use super::*;

    #[test]
    fn dpapi_blob_round_trip_is_atomic_and_not_plaintext() {
        let directory = tempfile::tempdir().unwrap();
        let store = SecureBlobStore::new(directory.path()).unwrap();
        let account = "profile-0-identity-42";
        let plaintext = "état ratchet strictement confidentiel";

        store
            .save(account, SecretString::from(plaintext.to_owned()))
            .unwrap();
        let protected = std::fs::read(store.path(account)).unwrap();
        assert!(
            !protected
                .windows(plaintext.len())
                .any(|part| part == plaintext.as_bytes())
        );
        let restored = store.load(account).unwrap().unwrap();
        assert_eq!(restored.expose_secret(), plaintext);

        store.remove(account).unwrap();
        assert!(store.load(account).unwrap().is_none());
    }
}

#[cfg(all(test, target_os = "linux"))]
mod linux_tests {
    use std::os::unix::fs::PermissionsExt as _;

    use secrecy::{ExposeSecret as _, SecretString};

    use super::*;

    #[test]
    fn encrypted_file_round_trip_uses_private_permissions_and_tombstone() {
        let temporary = tempfile::tempdir().unwrap();
        let store = SecureBlobStore::new(temporary.path()).unwrap();
        let account = "profile-0-github-session";
        let plaintext = "github-token-that-must-not-appear-on-disk";

        store
            .save(account, SecretString::from(plaintext.to_owned()))
            .unwrap();
        let protected = std::fs::read(store.path(account)).unwrap();
        assert!(
            !protected
                .windows(plaintext.len())
                .any(|part| part == plaintext.as_bytes())
        );
        let mode = std::fs::metadata(store.path(account))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(
            store.load(account).unwrap().unwrap().expose_secret(),
            plaintext
        );

        store.remove(account).unwrap();
        assert!(store.load(account).unwrap().is_none());
        assert!(store.tombstone_path(account).is_file());
    }

    #[test]
    fn account_name_is_authenticated_with_the_ciphertext() {
        let temporary = tempfile::tempdir().unwrap();
        let store = SecureBlobStore::new(temporary.path()).unwrap();
        store
            .save("account-one", SecretString::from("secret".to_owned()))
            .unwrap();
        std::fs::copy(store.path("account-one"), store.path("account-two")).unwrap();
        assert!(store.load("account-two").is_err());
    }
}
