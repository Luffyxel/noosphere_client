mod commands;
mod error;
mod github;
mod instance_profile;
mod models;
mod protocol;
mod repository_content;
mod secure_blob;
mod secure_store;
mod state;
mod validation;

use tauri::Manager as _;

#[cfg(target_os = "linux")]
type DesktopRuntime = tauri_runtime_cef::CefRuntime<tauri::EventLoopMessage>;

#[cfg(not(target_os = "linux"))]
type DesktopRuntime = tauri::Wry;

type DesktopAppHandle = tauri::AppHandle<DesktopRuntime>;

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
    let mut command_line_args = vec![("--disable-setuid-sandbox".into(), None)];
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
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
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
            commands::github_restore,
            commands::github_validate_session,
            commands::github_connect,
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
