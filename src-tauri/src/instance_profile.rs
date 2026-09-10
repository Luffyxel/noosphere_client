use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
};

use crate::error::{Error, Result};

pub(crate) const MAX_INSTANCE_PROFILES: u8 = 16;

pub struct InstanceProfile {
    slot: u8,
    base_directory: PathBuf,
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
                base_directory: base_directory.to_path_buf(),
                lock,
            });
        }
        Err(Error::TooManyInstances)
    }

    pub fn slot(&self) -> u8 {
        self.slot
    }

    pub fn base_directory(&self) -> &Path {
        &self.base_directory
    }

    pub fn directory_for_slot(&self, slot: u8) -> Result<PathBuf> {
        if slot >= MAX_INSTANCE_PROFILES {
            return Err(Error::InvalidData);
        }
        Ok(if slot == 0 {
            self.base_directory.clone()
        } else {
            self.base_directory
                .join("profiles")
                .join(format!("profile-{slot}"))
        })
    }

    pub fn secret_account(&self, kind: &str) -> Result<String> {
        Self::secret_account_for_slot(self.slot, kind)
    }

    pub fn secret_account_for_slot(slot: u8, kind: &str) -> Result<String> {
        if slot >= MAX_INSTANCE_PROFILES {
            return Err(Error::InvalidData);
        }
        if kind.is_empty()
            || kind.len() > 32
            || !kind
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(Error::InvalidData);
        }
        Ok(format!("profile-{slot}-{kind}"))
    }

    pub fn shared_account(&self, kind: &str) -> Result<String> {
        if kind.is_empty()
            || kind.len() > 48
            || !kind
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(Error::InvalidData);
        }
        Ok(format!("account-{kind}"))
    }

    pub fn acquire_account_lock(&self, repository_id: u64) -> Result<File> {
        if repository_id == 0 {
            return Err(Error::InvalidData);
        }
        let directory = self.base_directory.join("account-locks");
        create_private_directory(&directory)?;
        let lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(directory.join(format!("repository-{repository_id}.lock")))?;
        fs2::FileExt::try_lock_exclusive(&lock).map_err(|_| Error::AccountAlreadyOpen)?;
        Ok(lock)
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
        assert_ne!(
            first.directory_for_slot(first.slot()).unwrap(),
            second.directory_for_slot(second.slot()).unwrap()
        );
        assert_eq!(first.base_directory(), second.base_directory());
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
        assert_eq!(
            profile.shared_account("identity-123456789").unwrap(),
            "account-identity-123456789"
        );
    }

    #[test]
    fn one_repository_cannot_be_opened_by_two_instances() {
        let temporary = tempfile::tempdir().unwrap();
        let first = InstanceProfile::acquire(temporary.path()).unwrap();
        let second = InstanceProfile::acquire(temporary.path()).unwrap();
        let _account = first.acquire_account_lock(420).unwrap();

        assert!(matches!(
            second.acquire_account_lock(420),
            Err(Error::AccountAlreadyOpen)
        ));
    }
}
