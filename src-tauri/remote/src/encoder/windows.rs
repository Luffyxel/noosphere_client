//! Windows hardware media pipeline. Only compressed bytes leave GPU memory.
//! All COM objects are created and used on the media worker thread.
use super::VideoSettings;
use bytes::Bytes;
use std::{
    mem::ManuallyDrop,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Graphics::{Direct3D10::ID3D10Multithread, Direct3D11::*, Dxgi::Common::*},
        Media::MediaFoundation::*,
        System::{Com::*, Variant::VARIANT},
    },
    core::{GUID, Interface, Result},
};

pub fn media_error(error: windows::core::Error) -> crate::Error {
    crate::Error::Unavailable(format!("Windows media: {error}"))
}

pub struct Runtime {
    _dpi: crate::capture::dxgi::DpiContext,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}
impl Runtime {
    pub fn start() -> Result<Self> {
        let dpi = crate::capture::dxgi::DpiContext::enter()?;
        // SAFETY: the worker owns one COM/MF initialization pair and drops it last.
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
            if let Err(error) = MFStartup(MF_VERSION, MFSTARTUP_FULL) {
                CoUninitialize();
                return Err(error);
            }
        }
        Ok(Self {
            _dpi: dpi,
            _thread: std::marker::PhantomData,
        })
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
            CoUninitialize();
        }
    }
}

pub struct VideoProcessor {
    device: ID3D11Device,
    video: ID3D11VideoDevice,
    context: ID3D11VideoContext,
    enumerator: ID3D11VideoProcessorEnumerator,
    processor: ID3D11VideoProcessor,
    width: u32,
    height: u32,
}
impl VideoProcessor {
    pub fn new(
        device: &ID3D11Device,
        input: (u32, u32),
        output: (u32, u32),
        fps: u16,
    ) -> Result<Self> {
        unsafe {
            let video: ID3D11VideoDevice = device.cast()?;
            let context = device.GetImmediateContext()?.cast()?;
            let multithread: ID3D10Multithread = device.cast()?;
            let _ = multithread.SetMultithreadProtected(true);
            let enumerator =
                video.CreateVideoProcessorEnumerator(&D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
                    InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                    InputFrameRate: DXGI_RATIONAL {
                        Numerator: fps.into(),
                        Denominator: 1,
                    },
                    InputWidth: input.0,
                    InputHeight: input.1,
                    OutputFrameRate: DXGI_RATIONAL {
                        Numerator: fps.into(),
                        Denominator: 1,
                    },
                    OutputWidth: output.0,
                    OutputHeight: output.1,
                    Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
                })?;
            let processor = video.CreateVideoProcessor(&enumerator, 0)?;
            Ok(Self {
                device: device.clone(),
                video,
                context,
                enumerator,
                processor,
                width: output.0,
                height: output.1,
            })
        }
    }

    pub fn texture(&self, format: DXGI_FORMAT) -> Result<ID3D11Texture2D> {
        let mut texture = None;
        unsafe {
            self.device.CreateTexture2D(
                &D3D11_TEXTURE2D_DESC {
                    Width: self.width,
                    Height: self.height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: format,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
                    ..Default::default()
                },
                None,
                Some(&mut texture),
            )?;
        }
        texture.ok_or_else(|| {
            windows::core::Error::from_hresult(windows::Win32::Foundation::E_POINTER)
        })
    }

    pub fn blit(
        &self,
        input: &ID3D11Texture2D,
        index: u32,
        output: &ID3D11Texture2D,
    ) -> Result<()> {
        unsafe {
            let mut source = None;
            self.video.CreateVideoProcessorInputView(
                input,
                &self.enumerator,
                &D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
                    FourCC: 0,
                    ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
                    Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                        Texture2D: D3D11_TEX2D_VPIV {
                            MipSlice: 0,
                            ArraySlice: index,
                        },
                    },
                },
                Some(&mut source),
            )?;
            let mut target = None;
            self.video.CreateVideoProcessorOutputView(
                output,
                &self.enumerator,
                &D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                    ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                    Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                        Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
                    },
                },
                Some(&mut target),
            )?;
            self.context
                .VideoProcessorSetStreamAutoProcessingMode(&self.processor, 0, false);
            let mut stream = D3D11_VIDEO_PROCESSOR_STREAM {
                Enable: true.into(),
                pInputSurface: ManuallyDrop::new(source),
                ..Default::default()
            };
            let result = self.context.VideoProcessorBlt(
                &self.processor,
                target.as_ref().unwrap(),
                0,
                std::slice::from_ref(&stream),
            );
            ManuallyDrop::drop(&mut stream.pInputSurface);
            result
        }
    }
}

pub(crate) fn media_type(subtype: &GUID, settings: &VideoSettings) -> Result<IMFMediaType> {
    unsafe {
        let media = MFCreateMediaType()?;
        media.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        media.SetGUID(&MF_MT_SUBTYPE, subtype)?;
        media.SetUINT64(
            &MF_MT_FRAME_SIZE,
            ((settings.width as u64) << 32) | settings.height as u64,
        )?;
        media.SetUINT64(&MF_MT_FRAME_RATE, ((settings.fps as u64) << 32) | 1)?;
        media.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1u64 << 32) | 1)?;
        media.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        Ok(media)
    }
}

pub(crate) fn manager(device: &ID3D11Device) -> Result<IMFDXGIDeviceManager> {
    unsafe {
        let mut token = 0;
        let mut manager = None;
        MFCreateDXGIDeviceManager(&mut token, &mut manager)?;
        let manager = manager.unwrap();
        manager.ResetDevice(device, token)?;
        Ok(manager)
    }
}

/// Releases all COM references, including samples returned on an error status.
pub(crate) fn output(transform: &IMFTransform) -> Result<Option<IMFSample>> {
    unsafe {
        let info = transform.GetOutputStreamInfo(0)?;
        let mut buffer = MFT_OUTPUT_DATA_BUFFER::default();
        if info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 == 0 {
            let sample = MFCreateSample()?;
            sample.AddBuffer(&MFCreateMemoryBuffer(info.cbSize.max(1))?)?;
            buffer.pSample = ManuallyDrop::new(Some(sample));
        }
        let mut status = 0;
        let result = transform.ProcessOutput(0, std::slice::from_mut(&mut buffer), &mut status);
        let sample = ManuallyDrop::take(&mut buffer.pSample);
        ManuallyDrop::drop(&mut buffer.pEvents);
        match result {
            Ok(()) => Ok(sample),
            Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => Ok(None),
            Err(e) => Err(e),
        }
    }
}

pub struct EncodedFrame {
    pub bytes: Bytes,
    pub keyframe: bool,
    pub timestamp: i64,
}

#[windows::core::implement(IMFAsyncCallback)]
struct EncoderEvents {
    sender: std::sync::mpsc::SyncSender<IMFAsyncResult>,
}
impl IMFAsyncCallback_Impl for EncoderEvents_Impl {
    fn GetParameters(&self, _flags: *mut u32, _queue: *mut u32) -> Result<()> {
        Err(windows::core::Error::from_hresult(
            windows::Win32::Foundation::E_NOTIMPL,
        ))
    }
    fn Invoke(&self, result: windows::core::Ref<'_, IMFAsyncResult>) -> Result<()> {
        let result = result.as_ref().ok_or_else(|| {
            windows::core::Error::from_hresult(windows::Win32::Foundation::E_POINTER)
        })?;
        // One outstanding BeginGetEvent; never block a Media Foundation callback.
        let _ = self.sender.try_send(result.clone());
        Ok(())
    }
}

pub struct HardwareEncoder {
    transform: IMFTransform,
    events: IMFMediaEventGenerator,
    callback: IMFAsyncCallback,
    notifications: std::sync::mpsc::Receiver<IMFAsyncResult>,
    _manager: IMFDXGIDeviceManager,
    converter: VideoProcessor,
    duration: i64,
    input_credits: u32,
    pub name: String,
}
impl HardwareEncoder {
    pub fn open(
        device: &ID3D11Device,
        input: (u32, u32),
        settings: &VideoSettings,
    ) -> crate::Result<Self> {
        settings.validate()?;
        if settings.codec != super::Codec::H264 {
            return Err(crate::Error::Unavailable(
                "Windows native preview currently requires H.264".into(),
            ));
        }
        unsafe { Self::create(device, input, settings).map_err(media_error) }
    }

    unsafe fn create(
        device: &ID3D11Device,
        input: (u32, u32),
        settings: &VideoSettings,
    ) -> Result<Self> {
        // SAFETY: all calls use owned COM references on the initialized media worker.
        unsafe {
            let mut ptr = std::ptr::null_mut();
            let mut count = 0;
            MFTEnumEx(
                MFT_CATEGORY_VIDEO_ENCODER,
                MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
                None,
                Some(&MFT_REGISTER_TYPE_INFO {
                    guidMajorType: MFMediaType_Video,
                    guidSubtype: MFVideoFormat_H264,
                }),
                &mut ptr,
                &mut count,
            )?;
            let activations = if count == 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts_mut(ptr, count as usize)
                    .iter_mut()
                    .filter_map(Option::take)
                    .collect::<Vec<_>>()
            };
            CoTaskMemFree(Some(ptr.cast()));
            let manager = manager(device)?;
            let mut last = windows::core::Error::from_hresult(MF_E_TOPO_CODEC_NOT_FOUND);
            for activation in activations {
                let attempt = (|| -> Result<(IMFTransform, IMFMediaEventGenerator)> {
                    let transform: IMFTransform = activation.ActivateObject()?;
                    let attrs = transform.GetAttributes()?;
                    attrs.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1)?;
                    attrs.SetUINT32(&MF_LOW_LATENCY, 1)?;
                    if attrs.GetUINT32(&MF_SA_D3D11_AWARE).unwrap_or(0) == 0 {
                        return Err(windows::core::Error::from_hresult(
                            MF_E_UNSUPPORTED_D3D_TYPE,
                        ));
                    }
                    transform
                        .ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, manager.as_raw() as usize)?;
                    let out = media_type(&MFVideoFormat_H264, settings)?;
                    out.SetUINT32(&MF_MT_AVG_BITRATE, settings.bitrate as u32)?;
                    out.SetUINT32(&MF_MT_MPEG2_PROFILE, 77)?;
                    if let Ok(api) = transform.cast::<ICodecAPI>() {
                        let _ = api.SetValue(&CODECAPI_AVLowLatencyMode, &VARIANT::from(true));
                        let _ = api
                            .SetValue(&CODECAPI_AVEncMPVDefaultBPictureCount, &VARIANT::from(0u32));
                        let _ = api.SetValue(
                            &CODECAPI_AVEncMPVGOPSize,
                            &VARIANT::from(settings.fps as u32),
                        );
                    }
                    transform.SetOutputType(0, &out, 0)?;
                    transform.SetInputType(0, &media_type(&MFVideoFormat_NV12, settings)?, 0)?;
                    let events = transform.cast()?;
                    transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
                    transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
                    Ok((transform, events))
                })();
                match attempt {
                    Ok((transform, events)) => {
                        let (sender, notifications) = std::sync::mpsc::sync_channel(1);
                        let callback = EncoderEvents { sender }.into();
                        let mut name = [0u16; 256];
                        let mut length = 0;
                        activation.GetString(
                            &MFT_FRIENDLY_NAME_Attribute,
                            &mut name,
                            Some(&mut length),
                        )?;
                        return Ok(Self {
                            transform,
                            events,
                            callback,
                            notifications,
                            _manager: manager,
                            converter: VideoProcessor::new(
                                device,
                                input,
                                (settings.width, settings.height),
                                settings.fps,
                            )?,
                            duration: 10_000_000 / settings.fps as i64,
                            input_credits: 0,
                            name: String::from_utf16_lossy(&name[..length as usize]),
                        });
                    }
                    Err(error) => {
                        last = error;
                        let _ = activation.ShutdownObject();
                    }
                }
            }
            Err(last)
        }
    }

    pub fn set_bitrate(&mut self, bitrate: u64) -> crate::Result<()> {
        if !(500_000..=200_000_000).contains(&bitrate) {
            return Err(crate::Error::InvalidPacket);
        }
        unsafe {
            self.transform
                .cast::<ICodecAPI>()
                .and_then(|api| {
                    api.SetValue(
                        &CODECAPI_AVEncCommonMeanBitRate,
                        &VARIANT::from(bitrate as u32),
                    )
                })
                .map_err(media_error)
        }
    }

    pub fn encode(
        &mut self,
        texture: &ID3D11Texture2D,
        timestamp: i64,
        force_keyframe: bool,
    ) -> crate::Result<EncodedFrame> {
        unsafe {
            self.encode_inner(texture, timestamp, force_keyframe)
                .map_err(media_error)
        }
    }

    unsafe fn encode_inner(
        &mut self,
        texture: &ID3D11Texture2D,
        timestamp: i64,
        force_keyframe: bool,
    ) -> Result<EncodedFrame> {
        unsafe {
            let deadline = Instant::now() + Duration::from_millis(250);
            let mut submitted = false;
            loop {
                if Instant::now() >= deadline {
                    return Err(windows::core::Error::from_hresult(MF_E_NOTACCEPTING));
                }
                if !submitted && self.input_credits > 0 {
                    if force_keyframe {
                        self.transform
                            .cast::<ICodecAPI>()?
                            .SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &VARIANT::from(1u32))?;
                    }
                    let converted = self.converter.texture(DXGI_FORMAT_NV12)?;
                    self.converter.blit(texture, 0, &converted)?;
                    let sample = MFCreateSample()?;
                    sample.AddBuffer(&MFCreateDXGISurfaceBuffer(
                        &ID3D11Texture2D::IID,
                        &converted,
                        0,
                        false,
                    )?)?;
                    sample.SetSampleTime(timestamp)?;
                    sample.SetSampleDuration(self.duration)?;
                    self.transform.ProcessInput(0, &sample, 0)?;
                    submitted = true;
                    self.input_credits -= 1;
                }
                self.events.BeginGetEvent(&self.callback, None)?;
                let notification = self
                    .notifications
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .map_err(|_| windows::core::Error::from_hresult(MF_E_NOTACCEPTING))?;
                let event = self.events.EndGetEvent(&notification)?;
                event.GetStatus()?.ok()?;
                match event.GetType()? as i32 {
                    kind if kind == METransformNeedInput.0 => {
                        self.input_credits = self.input_credits.saturating_add(1);
                    }
                    kind if kind == METransformHaveOutput.0 => {
                        let result = match output(&self.transform) {
                            Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                                self.transform.SetOutputType(
                                    0,
                                    &self.transform.GetOutputAvailableType(0, 0)?,
                                    0,
                                )?;
                                continue;
                            }
                            result => result?,
                        };
                        if let Some(sample) = result {
                            let buffer = sample.ConvertToContiguousBuffer()?;
                            let mut data = std::ptr::null_mut();
                            let mut len = 0;
                            buffer.Lock(&mut data, None, Some(&mut len))?;
                            let bytes = if len as usize <= crate::transport::packet::MAX_FRAME {
                                Some(Bytes::copy_from_slice(std::slice::from_raw_parts(
                                    data,
                                    len as usize,
                                )))
                            } else {
                                None
                            };
                            buffer.Unlock()?;
                            return Ok(EncodedFrame {
                                bytes: bytes.ok_or_else(|| {
                                    windows::core::Error::from_hresult(MF_E_BUFFERTOOSMALL)
                                })?,
                                keyframe: sample
                                    .GetUINT32(&MFSampleExtension_CleanPoint)
                                    .unwrap_or(0)
                                    != 0,
                                timestamp: sample.GetSampleTime()?,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
impl Drop for HardwareEncoder {
    fn drop(&mut self) {
        unsafe {
            let _ = self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
            let _ = MFShutdownObject(&self.transform);
        }
    }
}
