// Copyright 2019-2024 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Adapter from CEF's permission callbacks to the runtime-neutral policy in
//! [`crate::policy`].
//!
//! Both handlers hand the policy an owned [`PermissionResponder`] holding a
//! reference-counted clone of the CEF callback, so a policy may answer now or
//! later (a native consent prompt) without the callback dying underneath it.
//! Every path — including a policy that panics its way out or drops the
//! responder — completes the callback exactly once, and only an explicit
//! verdict completes it with a grant.

use cef::{rc::Rc as _, *};

use crate::policy::{self, RequestSource};

wrap_permission_handler! {
  pub struct TauriCefPermissionHandler {
    webview_label: String,
  }

  impl PermissionHandler {
    fn on_request_media_access_permission(
      &self,
      _browser: Option<&mut Browser>,
      frame: Option<&mut Frame>,
      requesting_origin: Option<&CefString>,
      requested_permissions: u32,
      callback: Option<&mut MediaAccessCallback>,
    ) -> ::std::os::raw::c_int {
      let Some(callback) = callback else {
        return 0;
      };
      // Reference-counted clone: the callback outlives this stack frame when
      // the policy defers to a prompt.
      let callback = callback.clone();
      let origin = requesting_origin.map(|origin| origin.to_string()).unwrap_or_default();
      let is_main_frame = frame.map(|frame| frame.is_main() != 0);
      policy::dispatch(
        &self.webview_label,
        &origin,
        RequestSource::MediaAccess,
        policy::media_kinds(requested_permissions),
        is_main_frame,
        move |granted| {
          // getUserMedia requires the granted mask to equal the requested one
          // (cef_media_access_callback_t::cont), so this is all or nothing.
          callback.cont(if granted {
            requested_permissions
          } else {
            cef::sys::cef_media_access_permission_types_t::CEF_MEDIA_PERMISSION_NONE as u32
          });
        },
      );
      1
    }

    fn on_show_permission_prompt(
      &self,
      _browser: Option<&mut Browser>,
      _prompt_id: u64,
      requesting_origin: Option<&CefString>,
      requested_permissions: u32,
      callback: Option<&mut PermissionPromptCallback>,
    ) -> ::std::os::raw::c_int {
      let Some(callback) = callback else {
        return 0;
      };
      let callback = callback.clone();
      let origin = requesting_origin.map(|origin| origin.to_string()).unwrap_or_default();
      policy::dispatch(
        &self.webview_label,
        &origin,
        RequestSource::Prompt,
        policy::prompt_kinds(requested_permissions),
        // CEF reports no frame for permission prompts — they are browser-scoped.
        None,
        move |granted| {
          let result = if granted {
            cef::sys::cef_permission_request_result_t::CEF_PERMISSION_RESULT_ACCEPT
          } else {
            cef::sys::cef_permission_request_result_t::CEF_PERMISSION_RESULT_DENY
          };
          callback.cont(PermissionRequestResult::from(result));
        },
      );
      1
    }
  }
}
