use std::{
    fs,
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

const MAX_TOKEN_BYTES: u64 = 4096;

pub(super) struct TokenStore {
    path: PathBuf,
}

impl TokenStore {
    pub fn new(devices: Option<u32>) -> Option<Self> {
        let directory = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute())
                    .map(|path| path.join(".local/state"))
            })?;
        Some(Self::in_directory(
            &directory.join("noosphere/remote"),
            devices,
        ))
    }

    fn in_directory(directory: &Path, devices: Option<u32>) -> Self {
        let name = match devices {
            Some(devices) => format!("remote-desktop-{devices}.token"),
            None => "screencast.token".into(),
        };
        Self {
            path: directory.join(name),
        }
    }

    fn read(path: &Path) -> io::Result<Option<Zeroizing<String>>> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 {
            return Ok(None);
        }
        let mut token = Zeroizing::new(String::new());
        fs::File::open(path)?
            .take(MAX_TOKEN_BYTES + 1)
            .read_to_string(&mut token)?;
        if token.is_empty()
            || token.len() as u64 > MAX_TOKEN_BYTES
            || token.chars().any(char::is_control)
        {
            return Ok(None);
        }
        Ok(Some(token))
    }

    pub fn available(&self) -> bool {
        Self::read(&self.path).ok().flatten().is_some()
    }

    pub fn take(&self) -> Option<Zeroizing<String>> {
        let result = (|| {
            if !self.path.exists() {
                return Ok(None);
            }
            let temporary = tempfile::NamedTempFile::new_in(self.path.parent().unwrap())?;
            match fs::rename(&self.path, temporary.path()) {
                Ok(()) => Self::read(temporary.path()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error),
            }
        })();
        match result {
            Ok(token) => token,
            Err(error) => {
                eprintln!("[remote] lecture de l’autorisation de capture : {error}");
                None
            }
        }
    }

    fn write(&self, token: &str) -> io::Result<()> {
        if token.is_empty()
            || token.len() as u64 > MAX_TOKEN_BYTES
            || token.chars().any(char::is_control)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "jeton de capture invalide",
            ));
        }
        let directory = self.path.parent().unwrap();
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(directory)?;
        if !fs::symlink_metadata(directory)?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "répertoire de capture invalide",
            ));
        }
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
        let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        temporary.write_all(token.as_bytes())?;
        temporary.as_file().sync_all()?;
        temporary.persist(&self.path).map_err(|error| error.error)?;
        Ok(())
    }

    pub fn remember(&self, token: Option<&str>) {
        if let Some(token) = token
            && let Err(error) = self.write(token)
        {
            eprintln!("[remote] enregistrement de l’autorisation de capture : {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn capture_tokens_are_private_single_use_and_rotated() {
        let directory = tempfile::tempdir().unwrap();
        let store = TokenStore::in_directory(&directory.path().join("capture"), None);
        store.write("first-token").unwrap();
        assert_eq!(
            fs::metadata(store.path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&store.path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(store.available());
        assert_eq!(store.take().unwrap().as_str(), "first-token");
        assert!(store.take().is_none());
        store.write("next").unwrap();
        assert_eq!(store.take().unwrap().as_str(), "next");
    }

    #[test]
    fn input_permissions_and_portal_types_have_separate_tokens() {
        let directory = tempfile::tempdir().unwrap();
        let screen = TokenStore::in_directory(directory.path(), None);
        let pointer = TokenStore::in_directory(directory.path(), Some(2));
        let control = TokenStore::in_directory(directory.path(), Some(3));
        screen.write("screen").unwrap();
        control.write("control").unwrap();
        assert!(pointer.take().is_none());
        assert_eq!(screen.take().unwrap().as_str(), "screen");
        assert_eq!(control.take().unwrap().as_str(), "control");
    }

    #[test]
    fn untrusted_and_oversized_tokens_are_not_restored() {
        let directory = tempfile::tempdir().unwrap();
        let store = TokenStore::in_directory(directory.path(), None);
        store.write("private").unwrap();
        fs::set_permissions(&store.path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(store.take().is_none());
        let other = directory.path().join("other");
        fs::write(&other, "other-token").unwrap();
        symlink(&other, &store.path).unwrap();
        assert!(store.take().is_none());
        assert_eq!(fs::read_to_string(other).unwrap(), "other-token");
        assert!(
            store
                .write(&"a".repeat(MAX_TOKEN_BYTES as usize + 1))
                .is_err()
        );
        assert!(store.write("bad\ntoken").is_err());
    }
}
