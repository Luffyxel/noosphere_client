mod app_update;
mod background_host;
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
        if let Some(preload) = linux_nixos_va_preload(&driver_directory) {
            unsafe { std::env::set_var("NOOSPHERE_REMOTE_LD_PRELOAD", preload) };
        }
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
fn linux_nixos_va_preload(driver_directory: &std::path::Path) -> Option<std::ffi::OsString> {
    let driver = [
        "iHD_drv_video.so",
        "i965_drv_video.so",
        "radeonsi_drv_video.so",
        "nouveau_drv_video.so",
        "nvidia_drv_video.so",
    ]
    .into_iter()
    .map(|name| driver_directory.join(name))
    .find(|path| path.exists())?
    .canonicalize()
    .ok()?;
    let package = nix_store_package_for(&driver, std::path::Path::new("/nix/store"))?;
    let mut command = std::process::Command::new("/run/current-system/sw/bin/nix-store");
    command
        .args(["-q", "--references"])
        .arg(package)
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("LD_PRELOAD");
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let references = String::from_utf8(output.stdout)
        .ok()?
        .lines()
        .map(std::path::PathBuf::from)
        .collect::<Vec<_>>();
    nixos_va_preload_from_references(&references, std::env::var_os("LD_PRELOAD"))
}

#[cfg(target_os = "linux")]
fn nix_store_package_for(
    path: &std::path::Path,
    store: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let name = path.strip_prefix(store).ok()?.components().next()?;
    Some(store.join(name.as_os_str()))
}

#[cfg(target_os = "linux")]
fn nixos_va_preload_from_references(
    references: &[std::path::PathBuf],
    current: Option<std::ffi::OsString>,
) -> Option<std::ffi::OsString> {
    let va = references
        .iter()
        .map(|path| path.join("lib"))
        .find(|path| path.join("libva.so.2").is_file())?;
    let cxx = references
        .iter()
        .map(|path| path.join("lib/libstdc++.so.6"))
        .find(|path| path.is_file())?;
    let mut libraries = [
        "libva.so.2",
        "libva-drm.so.2",
        "libva-x11.so.2",
        "libva-wayland.so.2",
        "libva-glx.so.2",
    ]
    .into_iter()
    .map(|name| va.join(name))
    .filter(|path| path.is_file())
    .collect::<Vec<_>>();
    libraries.push(cxx);
    if let Some(current) = current {
        for path in std::env::split_paths(&current) {
            if !libraries.contains(&path) {
                libraries.push(path);
            }
        }
    }
    std::env::join_paths(libraries).ok()
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

    #[test]
    fn nix_store_package_is_derived_from_a_driver_symlink_target() {
        assert_eq!(
            nix_store_package_for(
                std::path::Path::new("/nix/store/driver-package/lib/dri/iHD_drv_video.so"),
                std::path::Path::new("/nix/store"),
            ),
            Some(std::path::PathBuf::from("/nix/store/driver-package"))
        );
    }

    #[test]
    fn nixos_va_preload_uses_one_compatible_library_set() {
        let temporary = tempfile::tempdir().unwrap();
        let va = temporary.path().join("libva");
        let cxx = temporary.path().join("gcc");
        std::fs::create_dir_all(va.join("lib")).unwrap();
        std::fs::create_dir_all(cxx.join("lib")).unwrap();
        for name in ["libva.so.2", "libva-drm.so.2", "libva-wayland.so.2"] {
            std::fs::write(va.join("lib").join(name), []).unwrap();
        }
        std::fs::write(cxx.join("lib/libstdc++.so.6"), []).unwrap();

        let value = nixos_va_preload_from_references(
            &[va.clone(), cxx.clone()],
            Some(std::ffi::OsString::from("/custom/libhook.so")),
        )
        .unwrap();
        let libraries = std::env::split_paths(&value).collect::<Vec<_>>();

        assert_eq!(
            libraries,
            [
                va.join("lib/libva.so.2"),
                va.join("lib/libva-drm.so.2"),
                va.join("lib/libva-wayland.so.2"),
                cxx.join("lib/libstdc++.so.6"),
                std::path::PathBuf::from("/custom/libhook.so"),
            ]
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
    let service_mode = background_host::service_mode();
    let smoke_mode = std::env::var("NOOSPHERE_SMOKE_TEST").as_deref() == Ok("1");
    let mut builder = tauri::Builder::<DesktopRuntime>::default();
    if !smoke_mode {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, arguments, _| {
            if arguments
                .iter()
                .any(|argument| argument == background_host::SERVICE_ARGUMENT)
            {
                return;
            }
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }));
    }
    builder
        .manage(background_host::BackgroundHost::new(service_mode))
        .manage(remote_access::RemoteAccess::default())
        .manage(remote_directory::RemoteDirectory::default())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            let data_directory = if smoke_mode {
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
            if service_mode && let Some(window) = app.get_webview_window("main") {
                window.hide()?;
            }
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    let background = handle.state::<background_host::BackgroundHost>();
                    let visible = handle
                        .get_webview_window("main")
                        .and_then(|window| window.is_visible().ok())
                        .unwrap_or(false);
                    if background.enabled() && !visible {
                        let state = handle.state::<state::AppState>();
                        if state.session.read().await.is_none() {
                            let _ = state.restore_session().await;
                        }
                        let viewer = state
                            .session
                            .read()
                            .await
                            .as_ref()
                            .map(|session| session.viewer.clone());
                        if let Some(viewer) = viewer {
                            if state.identity.read().await.is_none() {
                                let _ = state
                                    .load_or_create_identity(viewer.id, viewer.repository.id, None)
                                    .await;
                            }
                            if state.identity.read().await.is_some() {
                                let _ = remote_directory::remote_sync_directory(
                                    state,
                                    handle.state::<remote_directory::RemoteDirectory>(),
                                    handle.state::<remote_access::RemoteAccess>(),
                                    false,
                                )
                                .await;
                            }
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(4)).await;
                }
            });
            #[cfg(windows)]
            if let Some(window) = app.get_webview_window("main") {
                allow_local_media_permissions(&window)?;
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && window.state::<background_host::BackgroundHost>().enabled()
            {
                api.prevent_close();
                let _ = window.hide();
            }
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
