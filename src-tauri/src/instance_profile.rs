use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
};

use crate::error::{Error, Result};

const MAX_INSTANCE_PROFILES: u8 = 16;

pub struct InstanceProfile {
    slot: u8,
    directory: PathBuf,
    lock: File,
}

impl InstanceProfile {
    pub fn acquire(base_directory: &Path) -> Result<Self> {
        create_private_directory(base_directory)?;
        let lock_directory = base_directory.join("instance-locks");
        create_private_directory(&lock_directory)?;

        for slot in 0..MAX_INSTANCE_PROFILES {
            let lock_path = lock_directory.join(format!("profile-{slot}.lock"));
            let lock = OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .truncate(false)
                .open(lock_path)?;
            if fs2::FileExt::try_lock_exclusive(&lock).is_err() {
                continue;
            }
            let directory = if slot == 0 {
                base_directory.to_path_buf()
            } else {
                base_directory
                    .join("profiles")
                    .join(format!("profile-{slot}"))
            };
            create_private_directory(&directory)?;
            return Ok(Self {
                slot,
                directory,
                lock,
            });
        }
        Err(Error::TooManyInstances)
    }

    pub fn slot(&self) -> u8 {
        self.slot
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn secret_account(&self, kind: &str) -> Result<String> {
        if kind.is_empty()
            || kind.len() > 32
            || !kind
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(Error::InvalidData);
        }
        Ok(format!("profile-{}-{kind}", self.slot))
    }
}

impl Drop for InstanceProfile {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.lock);
    }
}

fn create_private_directory(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simultaneous_instances_are_isolated() {
        let temporary = tempfile::tempdir().unwrap();
        let first = InstanceProfile::acquire(temporary.path()).unwrap();
        let second = InstanceProfile::acquire(temporary.path()).unwrap();
        assert_eq!(first.slot(), 0);
        assert_eq!(second.slot(), 1);
        assert_ne!(first.directory(), second.directory());
    }

    #[test]
    fn released_slot_can_be_reused() {
        let temporary = tempfile::tempdir().unwrap();
        let first = InstanceProfile::acquire(temporary.path()).unwrap();
        assert_eq!(first.slot(), 0);
        drop(first);
        let replacement = InstanceProfile::acquire(temporary.path()).unwrap();
        assert_eq!(replacement.slot(), 0);
    }

    #[test]
    fn repository_scoped_secret_accounts_accept_numeric_ids() {
        let temporary = tempfile::tempdir().unwrap();
        let profile = InstanceProfile::acquire(temporary.path()).unwrap();

        assert_eq!(
            profile.secret_account("identity-123456789").unwrap(),
            "profile-0-identity-123456789"
        );
        assert_eq!(
            profile.secret_account("social-123456789").unwrap(),
            "profile-0-social-123456789"
        );
        assert!(profile.secret_account("identity/123456789").is_err());
        assert!(profile.secret_account("Identity-123456789").is_err());
    }
}
