use super::Event;
use crate::{Error, Result};
use windows::Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::Gdi::ClientToScreen,
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

pub const INPUT_MARKER: usize = 0x4e535249;

/// Windows desktop injection. Construct behind a granted InputController.
/// Live sessions map pointer coordinates onto the captured output.
pub struct NativeInput {
    display: Option<RECT>,
}

impl NativeInput {
    pub fn for_display(display: RECT) -> Self {
        Self {
            display: Some(display),
        }
    }

    fn desktop() -> Self {
        Self { display: None }
    }

    fn pointer(&self, x: u16, y: u16) -> Result<(i32, i32)> {
        let Some(display) = self.display else {
            return Ok((i32::from(x), i32::from(y)));
        };
        let _dpi = crate::capture::dxgi::DpiContext::enter()
            .map_err(crate::encoder::windows::media_error)?;
        let desktop = unsafe {
            let left = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let top = GetSystemMetrics(SM_YVIRTUALSCREEN);
            RECT {
                left,
                top,
                right: left + GetSystemMetrics(SM_CXVIRTUALSCREEN),
                bottom: top + GetSystemMetrics(SM_CYVIRTUALSCREEN),
            }
        };
        map_pointer(display, desktop, x, y)
    }
}

fn map_pointer(display: RECT, desktop: RECT, x: u16, y: u16) -> Result<(i32, i32)> {
    if display.right <= display.left
        || display.bottom <= display.top
        || desktop.right <= desktop.left
        || desktop.bottom <= desktop.top
        || display.left < desktop.left
        || display.top < desktop.top
        || display.right > desktop.right
        || display.bottom > desktop.bottom
    {
        return Err(Error::Unavailable(
            "L’écran partagé n’est plus disponible. Reconnectez la session.".into(),
        ));
    }
    let axis = |value: u16, start: i32, end: i32, origin: i32, limit: i32| {
        let pixel = i64::from(start)
            + i64::from(value) * (i64::from(end) - i64::from(start) - 1) / i64::from(u16::MAX);
        (((pixel - i64::from(origin)) * 65536 + 32768) / (i64::from(limit) - i64::from(origin)))
            .clamp(0, 65535) as i32
    };
    Ok((
        axis(x, display.left, display.right, desktop.left, desktop.right),
        axis(y, display.top, display.bottom, desktop.top, desktop.bottom),
    ))
}
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
            Event::Pointer { x, y } => {
                let (dx, dy) = self.pointer(*x, *y)?;
                INPUT {
                    r#type: INPUT_MOUSE,
                    Anonymous: INPUT_0 {
                        mi: MOUSEINPUT {
                            dx,
                            dy,
                            dwFlags: MOUSEEVENTF_MOVE
                                | MOUSEEVENTF_ABSOLUTE
                                | MOUSEEVENTF_VIRTUALDESK,
                            dwExtraInfo: INPUT_MARKER,
                            ..Default::default()
                        },
                    },
                }
            }
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
                NativeInput::desktop().inject(event)?;
                self.button_held = true;
                Ok(())
            },
            Event::Button {
                button: 0,
                pressed: false,
            } if self.button_held => {
                NativeInput::desktop().inject(event)?;
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
                NativeInput::desktop().inject(event)?;
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
            let _ = NativeInput::desktop().inject(&Event::Key {
                code: 135,
                pressed: false,
            });
        }
        if self.button_held {
            let _ = NativeInput::desktop().inject(&Event::Button {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    fn pixel(value: i32, origin: i32, extent: i32) -> i32 {
        origin + (i64::from(value) * i64::from(extent) / 65536) as i32
    }

    #[test]
    fn pointer_stays_on_the_captured_monitor_with_a_negative_desktop_origin() {
        let desktop = rect(-1920, 0, 2560, 1440);
        let display = rect(0, 0, 2560, 1440);
        for (x, y) in [(0, 0), (32768, 32768), (65535, 65535)] {
            let (dx, dy) = map_pointer(display, desktop, x, y).unwrap();
            let actual_x = pixel(dx, desktop.left, desktop.right - desktop.left);
            let actual_y = pixel(dy, desktop.top, desktop.bottom - desktop.top);
            let expected_x = i64::from(x) * 2559 / 65535;
            let expected_y = i64::from(y) * 1439 / 65535;
            assert_eq!(i64::from(actual_x), expected_x);
            assert_eq!(i64::from(actual_y), expected_y);
        }
    }

    #[test]
    fn pointer_handles_portrait_monitors_above_and_left_of_the_primary() {
        let desktop = rect(-1080, -1920, 3840, 2160);
        let display = rect(-1080, -1920, 0, 0);
        let (x, y) = map_pointer(display, desktop, 0, 0).unwrap();
        assert_eq!(pixel(x, desktop.left, 4920), display.left);
        assert_eq!(pixel(y, desktop.top, 4080), display.top);
        let (x, y) = map_pointer(display, desktop, 65535, 65535).unwrap();
        assert_eq!(pixel(x, desktop.left, 4920), -1);
        assert_eq!(pixel(y, desktop.top, 4080), -1);
    }

    #[test]
    fn full_desktop_endpoints_keep_their_normalized_range() {
        let desktop = rect(0, 0, 3840, 2160);
        let (x, y) = map_pointer(desktop, desktop, 0, 0).unwrap();
        assert_eq!((pixel(x, 0, 3840), pixel(y, 0, 2160)), (0, 0));
        let (x, y) = map_pointer(desktop, desktop, 65535, 65535).unwrap();
        assert_eq!((pixel(x, 0, 3840), pixel(y, 0, 2160)), (3839, 2159));
    }

    #[test]
    fn removed_or_invalid_outputs_do_not_inject_elsewhere() {
        let desktop = rect(0, 0, 1920, 1080);
        assert!(map_pointer(rect(-1920, 0, 0, 1080), desktop, 0, 0).is_err());
        assert!(map_pointer(rect(0, 0, 0, 1080), desktop, 0, 0).is_err());
    }
}
