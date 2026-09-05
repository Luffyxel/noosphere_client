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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
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
            commands::noosphere_poll_wake_signals,
            commands::noosphere_accept_friend_request,
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
