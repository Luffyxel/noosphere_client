use crate::encoder::{
    VideoSettings,
    windows::{manager, media_error, media_type, output},
};
use windows::{
    Win32::{
        Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D},
        Media::MediaFoundation::*,
        System::Com::*,
    },
    core::{Interface, Result},
};

pub struct DecodedFrame {
    pub texture: ID3D11Texture2D,
    pub index: u32,
    pub timestamp: i64,
    _sample: IMFSample,
}

pub struct HardwareDecoder {
    transform: IMFTransform,
    _manager: IMFDXGIDeviceManager,
}
impl HardwareDecoder {
    pub fn open(device: &ID3D11Device, settings: &VideoSettings) -> crate::Result<Self> {
        unsafe { Self::create(device, settings).map_err(media_error) }
    }
    unsafe fn create(device: &ID3D11Device, settings: &VideoSettings) -> Result<Self> {
        unsafe {
            let transform: IMFTransform =
                CoCreateInstance(&CLSID_MSH264DecoderMFT, None, CLSCTX_INPROC_SERVER)?;
            let attrs = transform.GetAttributes()?;
            attrs.SetUINT32(&MF_LOW_LATENCY, 1)?;
            let manager = manager(device)?;
            transform.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, manager.as_raw() as usize)?;
            transform.SetInputType(0, &media_type(&MFVideoFormat_H264, settings)?, 0)?;
            let decoder = Self {
                transform,
                _manager: manager,
            };
            decoder.select_output()?;
            decoder
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            decoder
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
            Ok(decoder)
        }
    }
    fn select_output(&self) -> Result<()> {
        unsafe {
            for index in 0..32 {
                let media = self.transform.GetOutputAvailableType(0, index)?;
                if media.GetGUID(&MF_MT_SUBTYPE)? == MFVideoFormat_NV12 {
                    return self.transform.SetOutputType(0, &media, 0);
                }
            }
            Err(windows::core::Error::from_hresult(MF_E_INVALIDMEDIATYPE))
        }
    }
    pub fn decode(&mut self, bytes: &[u8], timestamp: i64) -> crate::Result<Option<DecodedFrame>> {
        if bytes.is_empty() || bytes.len() > crate::transport::packet::MAX_FRAME {
            return Err(crate::Error::InvalidPacket);
        }
        unsafe { self.decode_inner(bytes, timestamp).map_err(media_error) }
    }
    unsafe fn decode_inner(
        &mut self,
        bytes: &[u8],
        timestamp: i64,
    ) -> Result<Option<DecodedFrame>> {
        unsafe {
            let sample = MFCreateSample()?;
            let buffer = MFCreateMemoryBuffer(bytes.len() as u32)?;
            let mut pointer = std::ptr::null_mut();
            buffer.Lock(&mut pointer, None, None)?;
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer, bytes.len());
            buffer.Unlock()?;
            buffer.SetCurrentLength(bytes.len() as u32)?;
            sample.AddBuffer(&buffer)?;
            sample.SetSampleTime(timestamp)?;
            self.transform.ProcessInput(0, &sample, 0)?;
            let result = match output(&self.transform) {
                Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                    self.select_output()?;
                    output(&self.transform)?
                }
                result => result?,
            };
            result
                .map(|sample| {
                    // Reject a software output: the renderer requires a real D3D11 surface.
                    let buffer: IMFDXGIBuffer = sample.GetBufferByIndex(0)?.cast()?;
                    let mut pointer = std::ptr::null_mut();
                    buffer.GetResource(&ID3D11Texture2D::IID, &mut pointer)?;
                    let texture = ID3D11Texture2D::from_raw(pointer);
                    Ok(DecodedFrame {
                        texture,
                        index: buffer.GetSubresourceIndex()?,
                        timestamp: sample.GetSampleTime()?,
                        _sample: sample,
                    })
                })
                .transpose()
        }
    }
}
impl Drop for HardwareDecoder {
    fn drop(&mut self) {
        unsafe {
            let _ = self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
        }
    }
}
