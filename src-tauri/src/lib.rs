mod app_update;
mod commands;
mod error;
mod github;
mod instance_profile;
mod models;
mod protocol;
mod remote_access;
mod remote_directory;
mod repository_content;
mod secure_blob;
#[cfg(windows)]
mod secure_store;
mod state;
mod validation;

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
pub fn appimage_relaunch_helper_status() -> Option<i32> {
    app_update::relaunch_helper_status()
}

#[cfg(windows)]
pub fn run_firewall_installer() -> i32 {
    remote_access::install_firewall_for_current_executable()
}

use tauri::Manager as _;

#[cfg(target_os = "linux")]
type DesktopRuntime = tauri_runtime_cef::CefRuntime<tauri::EventLoopMessage>;

#[cfg(not(target_os = "linux"))]
type DesktopRuntime = tauri::Wry;

type DesktopAppHandle = tauri::AppHandle<DesktopRuntime>;

#[cfg(target_os = "linux")]
pub fn prepare_linux_media_runtime() {
    let Some(app_dir) = std::env::var_os("APPDIR").map(std::path::PathBuf::from) else {
        return;
    };
    let runtime = app_dir.join("usr/lib/Noosphere/linux-media");
    if !runtime.join("gstreamer-1.0").is_dir() {
        return;
    }
    let variables = [
        ("GST_PLUGIN_SYSTEM_PATH_1_0", runtime.join("gstreamer-1.0")),
        ("GST_PLUGIN_SCANNER", runtime.join("gst-plugin-scanner")),
        ("SPA_PLUGIN_DIR", runtime.join("spa-0.2")),
        ("PIPEWIRE_MODULE_DIR", runtime.join("pipewire-0.3")),
        ("PIPEWIRE_CONFIG_DIR", runtime.join("pipewire")),
        ("XKB_CONFIG_ROOT", runtime.join("xkb")),
    ];
    for (name, path) in variables {
        if path.exists() {
            // This runs at process entry, before Tauri, Tokio, CEF or the host
            // daemon starts any threads. Child host processes inherit it.
            unsafe { std::env::set_var(name, path) };
        }
    }

    if let Some(driver_directory) = linux_va_driver_directory() {
        prepend_linux_path("LIBVA_DRIVERS_PATH", &driver_directory);
    }

    if std::env::var_os("GST_REGISTRY").is_none()
        && let Some(registry) = linux_media_registry_path()
        && let Some(parent) = registry.parent()
        && std::fs::create_dir_all(parent).is_ok()
    {
        // Keep the AppImage plugin registry separate from the host registry.
        // The versioned name also forces a fresh hardware probe after updates.
        unsafe { std::env::set_var("GST_REGISTRY", registry) };
    }
}

#[cfg(target_os = "linux")]
fn linux_va_driver_directory() -> Option<std::path::PathBuf> {
    first_linux_va_driver_directory([
        "/run/opengl-driver/lib/dri",
        "/usr/lib/x86_64-linux-gnu/dri",
        "/usr/lib64/dri",
        "/usr/lib/dri",
    ])
}

#[cfg(target_os = "linux")]
fn first_linux_va_driver_directory(
    candidates: impl IntoIterator<Item = impl AsRef<std::path::Path>>,
) -> Option<std::path::PathBuf> {
    candidates
        .into_iter()
        .map(|path| path.as_ref().to_path_buf())
        .find(|path| path.is_dir())
}

#[cfg(target_os = "linux")]
fn prepend_linux_path(name: &str, path: &std::path::Path) {
    if let Some(value) = prepend_linux_path_value(path, std::env::var_os(name)) {
        unsafe { std::env::set_var(name, value) };
    }
}

#[cfg(target_os = "linux")]
fn prepend_linux_path_value(
    path: &std::path::Path,
    current: Option<std::ffi::OsString>,
) -> Option<std::ffi::OsString> {
    let mut paths = vec![path.to_path_buf()];
    if let Some(current) = current {
        paths.extend(std::env::split_paths(&current));
    }
    paths.dedup();
    std::env::join_paths(paths).ok()
}

#[cfg(target_os = "linux")]
fn linux_media_registry_path() -> Option<std::path::PathBuf> {
    linux_media_registry_path_for(
        std::env::var_os("XDG_CACHE_HOME"),
        std::env::var_os("HOME"),
        env!("CARGO_PKG_VERSION"),
    )
}

#[cfg(target_os = "linux")]
fn linux_media_registry_path_for(
    cache_home: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
    version: &str,
) -> Option<std::path::PathBuf> {
    cache_home
        .filter(|path| !path.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            home.filter(|path| !path.is_empty())
                .map(std::path::PathBuf::from)
                .map(|path| path.join(".cache"))
        })
        .map(|path| {
            path.join("noosphere")
                .join(format!("gstreamer-registry-{version}.bin"))
        })
}

#[cfg(all(test, target_os = "linux"))]
mod linux_media_runtime_tests {
    use super::*;

    #[test]
    fn nixos_va_driver_directory_is_selected() {
        let temporary = tempfile::tempdir().unwrap();
        let missing = temporary.path().join("missing");
        let driver = temporary.path().join("run/opengl-driver/lib/dri");
        std::fs::create_dir_all(&driver).unwrap();

        assert_eq!(
            first_linux_va_driver_directory([missing, driver.clone()]),
            Some(driver)
        );
    }

    #[test]
    fn va_driver_path_keeps_existing_directories() {
        let value = prepend_linux_path_value(
            std::path::Path::new("/run/opengl-driver/lib/dri"),
            Some(std::ffi::OsString::from("/custom/dri:/usr/lib/dri")),
        )
        .unwrap();
        let paths = std::env::split_paths(&value).collect::<Vec<_>>();

        assert_eq!(
            paths,
            [
                std::path::PathBuf::from("/run/opengl-driver/lib/dri"),
                std::path::PathBuf::from("/custom/dri"),
                std::path::PathBuf::from("/usr/lib/dri"),
            ]
        );
    }

    #[test]
    fn gstreamer_registry_is_scoped_to_noosphere_version() {
        assert_eq!(
            linux_media_registry_path_for(
                Some(std::ffi::OsString::from("/tmp/cache")),
                Some(std::ffi::OsString::from("/home/test")),
                "0.1.43",
            ),
            Some(std::path::PathBuf::from(
                "/tmp/cache/noosphere/gstreamer-registry-0.1.43.bin"
            ))
        );
    }
}

#[cfg(windows)]
fn allow_local_media_permissions(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    window.with_webview(|platform_webview| unsafe {
        use webview2_com::{
            Microsoft::Web::WebView2::Win32::{
                COREWEBVIEW2_PERMISSION_KIND_CAMERA, COREWEBVIEW2_PERMISSION_KIND_MICROPHONE,
                COREWEBVIEW2_PERMISSION_STATE_ALLOW,
            },
            PermissionRequestedEventHandler,
        };

        let Ok(webview) = platform_webview.controller().CoreWebView2() else {
            return;
        };
        let handler = PermissionRequestedEventHandler::create(Box::new(|_, arguments| {
            let Some(arguments) = arguments else {
                return Ok(());
            };
            let mut permission = Default::default();
            arguments.PermissionKind(&mut permission)?;
            if permission == COREWEBVIEW2_PERMISSION_KIND_MICROPHONE
                || permission == COREWEBVIEW2_PERMISSION_KIND_CAMERA
            {
                arguments.SetState(COREWEBVIEW2_PERMISSION_STATE_ALLOW)?;
            }
            Ok(())
        }));
        let mut token = 0;
        let _ = webview.add_PermissionRequested(&handler, &mut token);
    })
}

#[cfg(target_os = "linux")]
fn configure_linux_runtime() -> bool {
    use tauri_runtime_cef::{DenyReason, PermissionKind, Verdict};

    let smoke_test = std::env::var("NOOSPHERE_SMOKE_TEST").as_deref() == Ok("1");
    let resource_directory = linux_cef_resource_directory();
    let mut command_line_args = vec![
        ("--disable-setuid-sandbox".into(), None),
        ("password-store".into(), Some("basic".into())),
    ];
    if smoke_test {
        command_line_args.extend([
            ("--no-sandbox".into(), None),
            ("--single-process".into(), None),
            ("--disable-gpu".into(), None),
            ("use-gl".into(), Some("angle".into())),
            ("use-angle".into(), Some("swiftshader".into())),
            ("--enable-unsafe-swiftshader".into(), None),
            ("--use-fake-device-for-media-stream".into(), None),
        ]);
    }
    let cache_path = smoke_test.then(|| {
        std::env::var_os("NOOSPHERE_SMOKE_USER_DATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("cef-{}", std::process::id()))
    });
    tauri_runtime_cef::configure(tauri_runtime_cef::CefConfig {
        identifier: "app.noosphere.client".into(),
        command_line_args,
        cache_path,
        locales_dir_path: Some(resource_directory.join("locales")),
        resources_dir_path: Some(resource_directory),
        ..Default::default()
    });
    if std::env::args().any(|argument| argument.starts_with("--type=")) {
        tauri_runtime_cef::run_cef_helper_process();
        return false;
    }
    tauri_runtime_cef::set_permission_policy(|request, responder| {
        let Some(origin) = request.origin.as_ref() else {
            return responder.deny(DenyReason::InvalidOrigin);
        };
        if request.webview_label != "main" || !origin.is_app_local() {
            return responder.deny(DenyReason::PolicyDenied);
        }
        let verdicts = request
            .kinds
            .iter()
            .map(|kind| match kind {
                PermissionKind::Microphone | PermissionKind::Camera => Verdict::Allow,
                _ => Verdict::Deny,
            })
            .collect();
        responder.decide(verdicts)
    });
    true
}

#[cfg(target_os = "linux")]
fn linux_cef_resource_directory() -> std::path::PathBuf {
    let executable_directory = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_else(std::env::temp_dir);
    if executable_directory.join("resources.pak").is_file() {
        return executable_directory;
    }
    let installed = executable_directory.join("../lib/Noosphere");
    if installed.join("resources.pak").is_file() {
        return installed;
    }
    std::env::var_os("APPDIR")
        .map(std::path::PathBuf::from)
        .map(|path| path.join("usr/lib/Noosphere"))
        .unwrap_or_else(|| std::path::PathBuf::from("/usr/lib/Noosphere"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    if !configure_linux_runtime() {
        return;
    }
    let builder = tauri::Builder::<DesktopRuntime>::default();
    builder
        .manage(remote_access::RemoteAccess::default())
        .manage(remote_directory::RemoteDirectory::default())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let data_directory = if std::env::var("NOOSPHERE_SMOKE_TEST").as_deref() == Ok("1") {
                let path = std::env::var_os("NOOSPHERE_SMOKE_USER_DATA")
                    .map(std::path::PathBuf::from)
                    .filter(|path| path.is_absolute())
                    .ok_or_else(|| std::io::Error::other("invalid smoke data directory"))?;
                if path.file_name().and_then(|value| value.to_str()) != Some("smoke-user-data") {
                    return Err(std::io::Error::other("invalid smoke data directory").into());
                }
                path
            } else {
                app.path().app_data_dir()?
            };
            let profile = instance_profile::InstanceProfile::acquire(&data_directory)?;
            app.manage(state::AppState::new(profile)?);
            #[cfg(windows)]
            if let Some(window) = app.get_webview_window("main") {
                allow_local_media_permissions(&window)?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_update::system_update_target,
            app_update::system_install_appimage_update,
            app_update::system_relaunch_after_update,
            remote_access::remote_status,
            remote_access::remote_save_settings,
            remote_access::remote_start_local_test,
            remote_access::remote_stop_local_test,
            remote_directory::remote_directory,
            remote_directory::remote_sync_directory,
            remote_directory::remote_set_host,
            remote_directory::remote_set_grant,
            remote_directory::remote_set_user_grant,
            remote_directory::remote_request_access,
            remote_directory::remote_connect_machine,
            remote_directory::remote_stop_session,
            remote_directory::remote_decide_access,
            remote_directory::remote_cancel_access,
            commands::github_restore,
            commands::github_validate_session,
            commands::github_connect,
            commands::github_open_device_page,
            commands::github_sign_out,
            commands::noosphere_lookup_user,
            commands::noosphere_send_friend_request,
            commands::noosphere_cached_state,
            commands::noosphere_sync_state,
            commands::noosphere_sync_requests,
            commands::noosphere_poll_wake_signals,
            commands::noosphere_accept_friend_request,
            commands::noosphere_decline_friend_request,
            commands::noosphere_remove_friend,
            commands::noosphere_send_message,
            commands::noosphere_signal_wake,
            commands::noosphere_signal_call,
            commands::noosphere_read_call_signal,
            commands::noosphere_list_messages,
            commands::noosphere_publish_realtime_signal,
            commands::noosphere_read_realtime_signal,
            commands::system_notify,
            commands::system_smoke_config,
            commands::system_smoke_complete,
        ])
        .run(tauri::generate_context!())
        .expect("Noosphere could not start");
}
