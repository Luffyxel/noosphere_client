#![doc = include_str!("../README.md")]

#[allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    clippy::all
)]
mod bindings;
pub use bindings::*;

#[cfg(target_os = "windows")]
impl Default for HWND {
    fn default() -> Self {
        Self(std::ptr::null_mut())
    }
}

#[cfg(target_os = "windows")]
impl Default for HINSTANCE {
    fn default() -> Self {
        Self(std::ptr::null_mut())
    }
}

#[cfg(target_os = "macos")]
pub const FRAMEWORK_PATH: &str =
    "Chromium Embedded Framework.framework/Chromium Embedded Framework";

use std::{
    env::{
        self,
        consts::{ARCH, OS},
    },
    fs,
    path::PathBuf,
};

pub fn get_cef_dir() -> Option<PathBuf> {
    let cef_path_env = env::var("FLATPAK")
        .map(|_| String::from("/usr/lib"))
        .or_else(|_| env::var("CEF_PATH"));

    match cef_path_env {
        Ok(cef_path) => {
            // Allow overriding the CEF path with environment variables.
            let configured_path = PathBuf::from(cef_path);
            let cef_dir = format!("cef_{OS}_{ARCH}");
            let package_version = env!("CARGO_PKG_VERSION");
            let cef_version = package_version
                .split_once('+')
                .map(|(_, version)| version)
                .unwrap_or(package_version);

            [
                configured_path.join(cef_version).join(&cef_dir),
                configured_path,
            ]
            .into_iter()
            .find_map(|path| fs::exists(&path).ok()?.then(|| path.canonicalize().ok()))
            .flatten()
        }
        Err(_) => {
            let out_dir = PathBuf::from(env!("OUT_DIR"));
            let cef_dir = format!("cef_{OS}_{ARCH}");
            let cef_dir = out_dir.join(&cef_dir).canonicalize().ok()?;
            fs::exists(&cef_dir).ok()?.then_some(cef_dir)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_cef_dir() {
        let _ = get_cef_dir().expect("CEF not found");
    }

    #[test]
    fn test_init() {
        use std::ptr::*;

        unsafe {
            #[cfg(target_os = "macos")]
            {
                use std::os::unix::ffi::OsStrExt;

                let cef_dir = get_cef_dir().expect("CEF not found");
                let framework_dir = cef_dir
                    .join(FRAMEWORK_PATH)
                    .canonicalize()
                    .expect("failed to get framework path");
                let framework_dir = std::ffi::CString::new(framework_dir.as_os_str().as_bytes())
                    .expect("invalid path");

                assert_eq!(cef_load_library(framework_dir.as_ptr().cast()), 1);
            }

            assert_eq!(cef_initialize(null(), null(), null_mut(), null_mut()), 0);
        };
    }
}
