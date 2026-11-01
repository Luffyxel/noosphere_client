use crate::{decoder::windows::DecodedFrame, encoder::windows::VideoProcessor};
use std::sync::Mutex;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::{
            Direct3D11::{ID3D11Device, ID3D11Texture2D},
            Dxgi::{Common::*, *},
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::*,
    },
    core::{Interface, PCWSTR, Result, w},
};

fn key_code(code: usize, flags: isize) -> u16 {
    let extended = flags & (1 << 24) != 0;
    match code {
        0x10 => {
            if (flags >> 16) & 0xff == 0x36 {
                0xA1
            } else {
                0xA0
            }
        }
        0x11 => {
            if extended {
                0xA3
            } else {
                0xA2
            }
        }
        0x12 => {
            if extended {
                0xA5
            } else {
                0xA4
            }
        }
        code => code as u16,
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize) };
    }
    let state = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState };
    if !state.is_null()
        && unsafe { GetMessageExtraInfo().0 as usize } != crate::input::windows::INPUT_MARKER
    {
        if message == WM_KILLFOCUS {
            let mut held = unsafe { &*state }
                .held
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let mut events = unsafe { &*state }
                .events
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            events.extend(held.release_all());
        }
        let event = match message {
            WM_KEYDOWN | WM_SYSKEYDOWN if wparam.0 < 256 && wparam.0 != 27 => {
                Some(crate::input::Event::Key {
                    code: key_code(wparam.0, lparam.0),
                    pressed: true,
                })
            }
            WM_KEYUP | WM_SYSKEYUP if wparam.0 < 256 => Some(crate::input::Event::Key {
                code: key_code(wparam.0, lparam.0),
                pressed: false,
            }),
            WM_LBUTTONDOWN | WM_LBUTTONUP => Some(crate::input::Event::Button {
                button: 0,
                pressed: message == WM_LBUTTONDOWN,
            }),
            WM_RBUTTONDOWN | WM_RBUTTONUP => Some(crate::input::Event::Button {
                button: 1,
                pressed: message == WM_RBUTTONDOWN,
            }),
            WM_MBUTTONDOWN | WM_MBUTTONUP => Some(crate::input::Event::Button {
                button: 2,
                pressed: message == WM_MBUTTONDOWN,
            }),
            WM_MOUSEMOVE => {
                let x = (lparam.0 & 0xffff) as i16 as i32;
                let y = ((lparam.0 >> 16) & 0xffff) as i16 as i32;
                let mut rect = RECT::default();
                if unsafe { GetClientRect(hwnd, &mut rect) }.is_ok() {
                    let width = (rect.right - rect.left - 1).max(1);
                    let height = (rect.bottom - rect.top - 1).max(1);
                    Some(crate::input::Event::Pointer {
                        x: (x.clamp(0, width) as u64 * 65_535 / width as u64) as u16,
                        y: (y.clamp(0, height) as u64 * 65_535 / height as u64) as u16,
                    })
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(event) = event {
            unsafe { &*state }
                .held
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .record(&event);
            let mut events = unsafe { &*state }
                .events
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if matches!(event, crate::input::Event::Pointer { .. })
                && events
                    .last()
                    .is_some_and(|last| matches!(last, crate::input::Event::Pointer { .. }))
            {
                events.pop();
            }
            if events.len() >= 512 {
                let index = events
                    .iter()
                    .position(|event| matches!(event, crate::input::Event::Pointer { .. }))
                    .unwrap_or(0);
                events.remove(index);
            }
            events.push(event);
        }
    }
    // SAFETY: parameters are provided by the Windows dispatcher; state lives in the worker.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

#[derive(Default)]
struct WindowState {
    held: Mutex<crate::input::InputState>,
    events: Mutex<Vec<crate::input::Event>>,
}

pub struct Viewer {
    hwnd: HWND,
    swap_chain: IDXGISwapChain1,
    converter: VideoProcessor,
    state: Box<WindowState>,
    pub observed_test_keys: u64,
    pub observed_test_buttons: u64,
}
impl Viewer {
    pub fn open(device: &ID3D11Device, width: u32, height: u32, fps: u16) -> Result<Self> {
        unsafe {
            let instance = GetModuleHandleW(None)?;
            let class = w!("NoosphereNativeRemoteViewer");
            // RegisterClass is process-local; subsequent sessions reuse the same class.
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                lpszClassName: class,
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                ..Default::default()
            });
            let mut state = Box::<WindowState>::default();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class,
                w!("Noosphere · Test local du bureau distant · Échap pour fermer"),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                960,
                580,
                None,
                None,
                Some(instance.into()),
                Some((&mut *state as *mut WindowState).cast()),
            )?;
            let attempt = (|| {
                let adapter = device.cast::<IDXGIDevice>()?.GetAdapter()?;
                let factory: IDXGIFactory2 = adapter.GetParent()?;
                let swap_chain = factory.CreateSwapChainForHwnd(
                    device,
                    hwnd,
                    &DXGI_SWAP_CHAIN_DESC1 {
                        Width: width,
                        Height: height,
                        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        SampleDesc: DXGI_SAMPLE_DESC {
                            Count: 1,
                            Quality: 0,
                        },
                        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                        BufferCount: 2,
                        Scaling: DXGI_SCALING_STRETCH,
                        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                        AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                        ..Default::default()
                    },
                    None,
                    None,
                )?;
                factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER)?;
                let latency: IDXGIDevice1 = device.cast()?;
                latency.SetMaximumFrameLatency(1)?;
                let converter = VideoProcessor::new(device, (width, height), (width, height), fps)?;
                Ok(Self {
                    hwnd,
                    swap_chain,
                    converter,
                    state,
                    observed_test_keys: 0,
                    observed_test_buttons: 0,
                })
            })();
            match attempt {
                Ok(viewer) => {
                    let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
                    // A hidden daemon's STARTUPINFO overrides the first ShowWindow.
                    // The viewer is an explicit user action, so show it after that override.
                    let _ = ShowWindow(hwnd, SW_SHOW);
                    let _ = SetForegroundWindow(hwnd);
                    Ok(viewer)
                }
                Err(e) => {
                    let _ = DestroyWindow(hwnd);
                    Err(e)
                }
            }
        }
    }
    pub fn pump(&mut self) -> bool {
        unsafe {
            let mut message = MSG::default();
            while PeekMessageW(&mut message, Some(self.hwnd), 0, 0, PM_REMOVE).as_bool() {
                if message.message == WM_KEYDOWN && message.wParam.0 == 27 {
                    let _ = DestroyWindow(self.hwnd);
                    return false;
                }
                if message.message == WM_KEYDOWN
                    && message.wParam.0 == 135
                    && GetMessageExtraInfo().0 as usize == crate::input::windows::INPUT_MARKER
                {
                    self.observed_test_keys += 1;
                }
                if message.message == WM_LBUTTONDOWN
                    && GetMessageExtraInfo().0 as usize == crate::input::windows::INPUT_MARKER
                {
                    self.observed_test_buttons += 1;
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            IsWindow(Some(self.hwnd)).as_bool()
        }
    }
    pub fn present(&self, frame: &DecodedFrame) -> Result<bool> {
        unsafe {
            let target: ID3D11Texture2D = self.swap_chain.GetBuffer(0)?;
            self.converter.blit(&frame.texture, frame.index, &target)?;
            let result = self.swap_chain.Present(0, DXGI_PRESENT_DO_NOT_WAIT);
            if result == DXGI_ERROR_WAS_STILL_DRAWING {
                return Ok(false);
            }
            result.ok()?;
            Ok(true)
        }
    }
    pub fn title(&self, text: &str) -> Result<()> {
        let wide: Vec<_> = text.encode_utf16().chain(Some(0)).collect();
        unsafe { SetWindowTextW(self.hwnd, PCWSTR(wide.as_ptr())) }
    }
    pub fn take_input(&self) -> Vec<crate::input::Event> {
        std::mem::take(
            &mut *self
                .state
                .events
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        )
    }
    pub fn test_target(&self) -> HWND {
        self.hwnd
    }
}
impl Drop for Viewer {
    fn drop(&mut self) {
        unsafe {
            if IsWindow(Some(self.hwnd)).as_bool() {
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestWindow(HWND);

    impl Drop for TestWindow {
        fn drop(&mut self) {
            let _ = unsafe { DestroyWindow(self.0) };
        }
    }

    #[test]
    fn native_mouse_messages_reach_the_remote_input_queue() {
        unsafe {
            let instance = GetModuleHandleW(None).unwrap();
            let class = w!("NoosphereMouseInputTest");
            assert_ne!(
                RegisterClassW(&WNDCLASSW {
                    lpfnWndProc: Some(window_proc),
                    hInstance: instance.into(),
                    lpszClassName: class,
                    ..Default::default()
                }),
                0
            );
            let mut state = Box::<WindowState>::default();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class,
                w!("Noosphere input test"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                400,
                300,
                None,
                None,
                Some(instance.into()),
                Some((&mut *state as *mut WindowState).cast()),
            )
            .unwrap();
            let _window = TestWindow(hwnd);
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect).unwrap();
            let point = LPARAM((((rect.bottom / 2) << 16) | (rect.right / 2)) as isize);
            SendMessageW(hwnd, WM_MOUSEMOVE, Some(WPARAM(0)), Some(point));
            SendMessageW(hwnd, WM_LBUTTONDOWN, Some(WPARAM(0)), Some(point));
            SendMessageW(hwnd, WM_LBUTTONUP, Some(WPARAM(0)), Some(point));
            let events = std::mem::take(&mut *state.events.lock().unwrap());
            assert_eq!(events.len(), 3, "{events:?}");
            let crate::input::Event::Pointer { x, y } = events[0] else {
                panic!("missing pointer event")
            };
            assert!((i32::from(x) - 32768).abs() < 256, "{x}");
            assert!((i32::from(y) - 32768).abs() < 256, "{y}");
            assert_eq!(
                events[1],
                crate::input::Event::Button {
                    button: 0,
                    pressed: true
                }
            );
            assert_eq!(
                events[2],
                crate::input::Event::Button {
                    button: 0,
                    pressed: false
                }
            );
            SendMessageW(hwnd, WM_RBUTTONDOWN, Some(WPARAM(0)), Some(point));
            SendMessageW(hwnd, WM_KILLFOCUS, Some(WPARAM(0)), Some(LPARAM(0)));
            let events = std::mem::take(&mut *state.events.lock().unwrap());
            assert_eq!(
                events,
                vec![
                    crate::input::Event::Button {
                        button: 1,
                        pressed: true
                    },
                    crate::input::Event::Button {
                        button: 1,
                        pressed: false
                    },
                ]
            );
        }
    }

    #[test]
    fn modifier_events_preserve_the_left_and_right_keys() {
        assert_eq!(key_code(0x10, 0x2a << 16), 0xA0);
        assert_eq!(key_code(0x10, 0x36 << 16), 0xA1);
        assert_eq!(key_code(0x11, 0), 0xA2);
        assert_eq!(key_code(0x11, 1 << 24), 0xA3);
        assert_eq!(key_code(0x12, 0), 0xA4);
        assert_eq!(key_code(0x12, 1 << 24), 0xA5);
        assert_eq!(key_code(0x41, 0), 0x41);
    }
}
