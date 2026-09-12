use super::Event;
use crate::{Error, Result};
use windows::Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::Gdi::ClientToScreen,
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

pub const INPUT_MARKER: usize = 0x4e535249;

/// Windows desktop injection. Construct behind a granted InputController; the
/// pointer coordinates address the normalized virtual desktop, not a WebView.
pub struct NativeInput;
impl super::InputSink for NativeInput {
    fn inject(&mut self, event: &Event) -> Result<()> {
        let input = match event {
            Event::Key { code, pressed } if *code < 256 => INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(*code),
                        dwFlags: if *pressed {
                            KEYBD_EVENT_FLAGS(0)
                        } else {
                            KEYEVENTF_KEYUP
                        },
                        dwExtraInfo: INPUT_MARKER,
                        ..Default::default()
                    },
                },
            },
            Event::Pointer { x, y } => INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: (*x).into(),
                        dy: (*y).into(),
                        dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                        dwExtraInfo: INPUT_MARKER,
                        ..Default::default()
                    },
                },
            },
            Event::Button { button, pressed } if *button < 5 => {
                let (down, up, data) = match button {
                    0 => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, 0),
                    1 => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, 0),
                    2 => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, 0),
                    3 => (MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, 1),
                    _ => (MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, 2),
                };
                INPUT {
                    r#type: INPUT_MOUSE,
                    Anonymous: INPUT_0 {
                        mi: MOUSEINPUT {
                            mouseData: data,
                            dwFlags: if *pressed { down } else { up },
                            dwExtraInfo: INPUT_MARKER,
                            ..Default::default()
                        },
                    },
                }
            }
            Event::Gamepad { buttons: 0 } => return Ok(()),
            _ => {
                return Err(Error::Unavailable(
                    "Unsupported Windows input device".into(),
                ));
            }
        };
        if unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) } != 1 {
            return Err(Error::Unavailable("Windows input injection failed".into()));
        }
        Ok(())
    }
}

pub struct TestInput {
    target: HWND,
    held: bool,
    button_held: bool,
    cursor: Option<(POINT, POINT)>,
}
impl TestInput {
    pub fn new(target: HWND) -> Self {
        Self {
            target,
            held: false,
            button_held: false,
            cursor: None,
        }
    }
}
impl super::InputSink for TestInput {
    fn inject(&mut self, event: &Event) -> Result<()> {
        match event {
            Event::Pointer { x, y } => unsafe {
                if GetForegroundWindow() != self.target {
                    return Err(Error::PermissionDenied);
                }
                let mut rect = RECT::default();
                let mut original = POINT::default();
                GetClientRect(self.target, &mut rect)
                    .map_err(crate::encoder::windows::media_error)?;
                GetCursorPos(&mut original).map_err(crate::encoder::windows::media_error)?;
                let mut point = POINT {
                    x: (i64::from(*x) * i64::from((rect.right - 1).max(0)) / 65535) as i32,
                    y: (i64::from(*y) * i64::from((rect.bottom - 1).max(0)) / 65535) as i32,
                };
                if !ClientToScreen(self.target, &mut point).as_bool()
                    || WindowFromPoint(point) != self.target
                {
                    return Err(Error::PermissionDenied);
                }
                SetCursorPos(point.x, point.y).map_err(crate::encoder::windows::media_error)?;
                self.cursor = Some((original, point));
                Ok(())
            },
            Event::Button {
                button: 0,
                pressed: true,
            } => unsafe {
                let mut point = POINT::default();
                GetCursorPos(&mut point).map_err(crate::encoder::windows::media_error)?;
                if GetForegroundWindow() != self.target
                    || WindowFromPoint(point) != self.target
                    || self.cursor.is_none()
                {
                    return Err(Error::PermissionDenied);
                }
                NativeInput.inject(event)?;
                self.button_held = true;
                Ok(())
            },
            Event::Button {
                button: 0,
                pressed: false,
            } if self.button_held => {
                NativeInput.inject(event)?;
                self.button_held = false;
                Ok(())
            }
            Event::Button {
                button: 0,
                pressed: false,
            } => Ok(()),
            Event::Key {
                code: 135,
                pressed: true,
            } => {
                inject_test(self.target, event)?;
                self.held = true;
                Ok(())
            }
            Event::Key {
                code: 135,
                pressed: false,
            } if self.held => {
                // Release our own key even after focus changes or a key-up datagram is lost.
                NativeInput.inject(event)?;
                self.held = false;
                Ok(())
            }
            Event::Key {
                code: 135,
                pressed: false,
            }
            | Event::Gamepad { buttons: 0 } => Ok(()),
            _ => Err(Error::PermissionDenied),
        }
    }
}

impl Drop for TestInput {
    fn drop(&mut self) {
        use super::InputSink as _;
        if self.held {
            let _ = NativeInput.inject(&Event::Key {
                code: 135,
                pressed: false,
            });
        }
        if self.button_held {
            let _ = NativeInput.inject(&Event::Button {
                button: 0,
                pressed: false,
            });
        }
        if let Some((original, placed)) = self.cursor {
            // Do not override a pointer movement made by the user during the test.
            let mut current = POINT::default();
            unsafe {
                if GetCursorPos(&mut current).is_ok() && current == placed {
                    let _ = SetCursorPos(original.x, original.y);
                }
            }
        }
    }
}

/// The local diagnostic can only inject into its own foreground viewer. WAN callers
/// must additionally pass every event through InputGuard and the current grant.
pub fn inject_test(target: HWND, event: &Event) -> Result<()> {
    unsafe {
        if GetForegroundWindow() != target {
            return Err(Error::PermissionDenied);
        }
    }
    let input = match event {
        Event::Key { code: 135, pressed } => INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(135),
                    dwFlags: if *pressed {
                        KEYBD_EVENT_FLAGS(0)
                    } else {
                        KEYEVENTF_KEYUP
                    },
                    dwExtraInfo: INPUT_MARKER,
                    ..Default::default()
                },
            },
        },
        // Wheel, shortcuts and arbitrary keyboard input are deliberately unavailable in self-test.
        _ => return Err(Error::PermissionDenied),
    };
    if unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) } != 1 {
        return Err(Error::Unavailable("Windows input injection failed".into()));
    }
    Ok(())
}
