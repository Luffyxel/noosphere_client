#[cfg(windows)]
pub mod dxgi;
#[cfg(target_os = "linux")]
pub mod linux;

use crate::Result;

pub trait Capture {
    type Surface;
    fn acquire(&mut self, timeout_ms: u32) -> Result<Option<Self::Surface>>;
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Display {
    pub id: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
}
