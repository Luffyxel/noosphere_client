use super::{Capture, Display};
use crate::{Error, Result};
use windows::{
    Win32::Graphics::{
        Direct3D::D3D_DRIVER_TYPE_HARDWARE,
        Direct3D11::{
            D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
            ID3D11Texture2D,
        },
        Dxgi::{
            DXGI_ERROR_NOT_FOUND, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO, IDXGIAdapter,
            IDXGIDevice, IDXGIOutput1, IDXGIOutputDuplication,
        },
    },
    core::Interface as _,
};

/// DXGI coordinates must use physical pixels, even when Windows scales the UI.
pub(crate) struct DpiContext(windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT);
impl DpiContext {
    pub(crate) fn enter() -> windows::core::Result<Self> {
        use windows::Win32::UI::HiDpi::*;
        let previous =
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        if previous.0.is_null() {
            return Err(windows::core::Error::from_win32());
        }
        Ok(Self(previous))
    }
}
impl Drop for DpiContext {
    fn drop(&mut self) {
        unsafe {
            windows::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(self.0);
        }
    }
}

fn error(error: windows::core::Error) -> Error {
    Error::Unavailable(format!("DXGI: {error}"))
}

pub(crate) fn device() -> Result<(ID3D11Device, IDXGIAdapter)> {
    let mut device = None;
    // SAFETY: COM output pointers are initialized and their owned wrappers release references.
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            Default::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )
        .map_err(error)?;
        let device = device.ok_or_else(|| Error::Unavailable("D3D11 device".into()))?;
        let dxgi: IDXGIDevice = device.cast().map_err(error)?;
        let adapter = dxgi.GetAdapter().map_err(error)?;
        Ok((device, adapter))
    }
}

pub fn displays() -> Result<Vec<Display>> {
    let _dpi = DpiContext::enter().map_err(error)?;
    let (_, adapter) = device()?;
    let mut displays = Vec::new();
    for id in 0..32 {
        // SAFETY: adapter is an owned COM interface; descriptions are returned by value.
        let output = match unsafe { adapter.EnumOutputs(id) } {
            Ok(output) => output,
            Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
            Err(e) => return Err(error(e)),
        };
        let desc = unsafe { output.GetDesc() }.map_err(error)?;
        if !desc.AttachedToDesktop.as_bool() {
            continue;
        }
        let name_len = desc
            .DeviceName
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(desc.DeviceName.len());
        displays.push(Display {
            id,
            name: String::from_utf16_lossy(&desc.DeviceName[..name_len]),
            width: (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left).max(0) as u32,
            height: (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top).max(0) as u32,
        });
    }
    Ok(displays)
}

pub struct DesktopDuplication {
    duplication: IDXGIOutputDuplication,
    pub device: ID3D11Device,
}

pub struct GpuFrame {
    pub texture: ID3D11Texture2D,
    pub presented_ticks: i64,
    duplication: IDXGIOutputDuplication,
}

impl Drop for GpuFrame {
    fn drop(&mut self) {
        // SAFETY: every successful acquisition creates exactly one guard owning ReleaseFrame.
        let _ = unsafe { self.duplication.ReleaseFrame() };
    }
}

impl GpuFrame {
    pub fn dimensions(&self) -> (u32, u32) {
        let mut desc = Default::default();
        unsafe {
            self.texture.GetDesc(&mut desc);
        }
        (desc.Width, desc.Height)
    }
}

impl DesktopDuplication {
    pub fn open(display: u32) -> Result<Self> {
        let _dpi = DpiContext::enter().map_err(error)?;
        let (device, adapter) = device()?;
        // SAFETY: device and output refer to the same adapter and remain alive during duplication.
        let duplication = unsafe {
            let output: IDXGIOutput1 = adapter
                .EnumOutputs(display)
                .map_err(error)?
                .cast()
                .map_err(error)?;
            output.DuplicateOutput(&device).map_err(error)?
        };
        Ok(Self {
            duplication,
            device,
        })
    }
}

impl Capture for DesktopDuplication {
    type Surface = GpuFrame;
    fn acquire(&mut self, timeout_ms: u32) -> Result<Option<GpuFrame>> {
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource = None;
        // SAFETY: initialized outputs are exclusively borrowed. Timeout is bounded to avoid blocking shutdown.
        match unsafe {
            self.duplication
                .AcquireNextFrame(timeout_ms.min(100), &mut info, &mut resource)
        } {
            Ok(()) => {}
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(None),
            Err(e) => return Err(error(e)),
        }
        let texture = resource
            .ok_or_else(|| Error::Unavailable("DXGI frame".into()))
            .and_then(|resource| resource.cast().map_err(error));
        match texture {
            Ok(texture) => Ok(Some(GpuFrame {
                texture,
                presented_ticks: info.LastPresentTime,
                duplication: self.duplication.clone(),
            })),
            Err(e) => {
                unsafe { self.duplication.ReleaseFrame() }.map_err(error)?;
                Err(e)
            }
        }
    }
}
