use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;

#[cfg(windows)]
use secrecy::ExposeSecret as _;
use secrecy::SecretString;

use crate::error::{Error, Result};
#[cfg(not(windows))]
use crate::secure_store::{SecretStore as _, SystemSecretStore};

#[cfg(windows)]
const MAX_PROTECTED_BYTES: usize = 32 * 1024 * 1024;

pub struct SecureBlobStore {
    #[cfg(windows)]
    directory: PathBuf,
    #[cfg(not(windows))]
    system: SystemSecretStore,
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
        Ok(Self {
            #[cfg(windows)]
            directory,
            #[cfg(not(windows))]
            system: SystemSecretStore,
        })
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
        #[cfg(not(windows))]
        self.system.load(account)
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
        #[cfg(not(windows))]
        self.system.save(account, value)
    }

    #[cfg(all(test, windows))]
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
        #[cfg(not(windows))]
        self.system.remove(account)
    }

    #[cfg(windows)]
    fn path(&self, account: &str) -> PathBuf {
        self.directory.join(format!("{account}.bin"))
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
