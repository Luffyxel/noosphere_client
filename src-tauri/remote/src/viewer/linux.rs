//! Native Linux viewer backed by GStreamer. Decoder selection stays inside
//! GStreamer so VA-API, NVDEC and software fallbacks can be chosen at runtime.
use crate::{Error, Result, encoder::VideoSettings, input::Event};
use bytes::Bytes;
use gst::prelude::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

fn viewer_error(context: &str, error: impl std::fmt::Display) -> Error {
    Error::Unavailable(format!("Linux · {context} : {error}"))
}

pub struct Viewer {
    pipeline: gst::Pipeline,
    source: gst_app::AppSrc,
    input: Arc<Mutex<Vec<Event>>>,
    headless_sink: Option<gst_app::AppSink>,
    decoded_frames: AtomicU64,
    frame_duration: gst::ClockTime,
    next_pts: u64,
    startup_frame: Option<Bytes>,
    startup_retry_at: Option<Instant>,
    startup_retries: u8,
}

impl Viewer {
    pub fn open(settings: &VideoSettings) -> Result<Self> {
        Self::open_with_sink(settings, false)
    }

    pub fn open_headless(settings: &VideoSettings) -> Result<Self> {
        Self::open_with_sink(settings, true)
    }

    fn open_with_sink(settings: &VideoSettings, headless: bool) -> Result<Self> {
        settings.validate()?;
        gst::init().map_err(|error| viewer_error("initialisation du viewer", error))?;
        let sink = if headless || std::env::var_os("NOOSPHERE_REMOTE_TEST_SINK").is_some() {
            "appsink name=noosphere_video_sink sync=false async=false max-buffers=1 drop=true enable-last-sample=false"
        } else {
            "autovideosink name=noosphere_video_sink sync=false"
        };
        let description = format!(
            "appsrc name=noosphere_source is-live=true format=time do-timestamp=false block=false max-buffers=4 ! h264parse ! queue leaky=downstream max-size-buffers=2 max-size-bytes=0 max-size-time=0 ! decodebin ! videoconvert ! {sink}"
        );
        let pipeline = gst::parse::launch(&description)
            .map_err(|error| viewer_error("création du viewer", error))?
            .downcast::<gst::Pipeline>()
            .map_err(|_| viewer_error("création du viewer", "pipeline invalide"))?;
        let source = pipeline
            .by_name("noosphere_source")
            .ok_or_else(|| viewer_error("viewer", "entrée vidéo absente"))?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| viewer_error("viewer", "entrée vidéo invalide"))?;
        source.set_caps(Some(
            &gst::Caps::builder("video/x-h264")
                .field("stream-format", "byte-stream")
                .field("alignment", "au")
                .build(),
        ));
        let headless_sink = headless
            .then(|| pipeline.by_name("noosphere_video_sink"))
            .flatten()
            .and_then(|sink| sink.downcast::<gst_app::AppSink>().ok());
        let input = Arc::new(Mutex::new(Vec::new()));
        if !headless
            && std::env::var_os("NOOSPHERE_REMOTE_TEST_SINK").is_none()
            && let Some(pad) = source.static_pad("src")
        {
            let events = input.clone();
            let width = settings.width;
            let height = settings.height;
            pad.add_probe(gst::PadProbeType::EVENT_UPSTREAM, move |_, info| {
                if let Some(gst::PadProbeData::Event(event)) = &info.data
                    && let Ok(navigation) = gst_video::NavigationEvent::parse(event)
                    && let Some(event) = navigation_event(navigation, width, height)
                {
                    events
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .push(event);
                }
                gst::PadProbeReturn::Ok
            });
        }
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|error| viewer_error("démarrage du viewer", error))?;
        Ok(Self {
            pipeline,
            source,
            input,
            headless_sink,
            decoded_frames: AtomicU64::new(0),
            frame_duration: gst::ClockTime::from_nseconds(1_000_000_000 / u64::from(settings.fps)),
            next_pts: 0,
            startup_frame: None,
            startup_retry_at: None,
            startup_retries: 0,
        })
    }

    pub fn present(&mut self, bytes: Bytes) -> Result<bool> {
        if self.next_pts == 0 {
            self.startup_frame = Some(bytes.clone());
            self.startup_retry_at = Some(Instant::now() + Duration::from_millis(40));
        } else {
            // A live stream already supplied the decoder with another access
            // unit, so replaying the first keyframe would only add jitter.
            self.startup_frame = None;
            self.startup_retry_at = None;
        }
        self.push(bytes)?;
        Ok(true)
    }

    fn push(&mut self, bytes: Bytes) -> Result<()> {
        let mut buffer = gst::Buffer::from_mut_slice(bytes.to_vec());
        if let Some(buffer) = buffer.get_mut() {
            buffer.set_pts(gst::ClockTime::from_nseconds(self.next_pts));
            buffer.set_duration(self.frame_duration);
        }
        self.next_pts = self.next_pts.saturating_add(self.frame_duration.nseconds());
        self.source
            .push_buffer(buffer)
            .map_err(|error| viewer_error("décodage de l’image", error))?;
        Ok(())
    }

    pub fn pump(&mut self) -> Result<bool> {
        if let Some(sink) = &self.headless_sink {
            while sink.try_pull_sample(gst::ClockTime::ZERO).is_some() {
                self.decoded_frames.fetch_add(1, Ordering::Relaxed);
            }
        }
        if self.decoded_frames.load(Ordering::Relaxed) > 0 {
            self.startup_frame = None;
            self.startup_retry_at = None;
        } else if self.startup_retries < 3
            && self
                .startup_retry_at
                .is_some_and(|deadline| Instant::now() >= deadline)
            && let Some(frame) = self.startup_frame.clone()
        {
            self.push(frame)?;
            self.startup_retries += 1;
            if self.startup_retries < 3 {
                self.startup_retry_at = Some(Instant::now() + Duration::from_millis(40));
            } else {
                self.startup_frame = None;
                self.startup_retry_at = None;
            }
        }
        let Some(bus) = self.pipeline.bus() else {
            return Ok(true);
        };
        let Some(message) = bus.timed_pop_filtered(
            gst::ClockTime::ZERO,
            &[gst::MessageType::Error, gst::MessageType::Eos],
        ) else {
            return Ok(true);
        };
        match message.view() {
            gst::MessageView::Error(error) => Err(viewer_error(
                "viewer",
                format!(
                    "{} ({})",
                    error.error(),
                    error.debug().unwrap_or_else(|| "sans détail".into())
                ),
            )),
            _ => Ok(false),
        }
    }

    pub fn take_input(&self) -> Vec<Event> {
        std::mem::take(&mut *self.input.lock().unwrap_or_else(|error| error.into_inner()))
    }

    pub fn decoded_frames(&self) -> u64 {
        self.decoded_frames.load(Ordering::Relaxed)
    }
}

impl Drop for Viewer {
    fn drop(&mut self) {
        let _ = self.source.end_of_stream();
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

fn normalized(value: f64, extent: u32) -> u16 {
    if !value.is_finite() || extent == 0 {
        return 0;
    }
    (value.clamp(0.0, f64::from(extent)) * f64::from(u16::MAX) / f64::from(extent)).round() as u16
}

fn navigation_event(event: gst_video::NavigationEvent, width: u32, height: u32) -> Option<Event> {
    match event {
        gst_video::NavigationEvent::KeyPress { key } => {
            key_to_windows_vk(&key).map(|code| Event::Key {
                code,
                pressed: true,
            })
        }
        gst_video::NavigationEvent::KeyRelease { key } => {
            key_to_windows_vk(&key).map(|code| Event::Key {
                code,
                pressed: false,
            })
        }
        gst_video::NavigationEvent::MouseMove { x, y } => Some(Event::Pointer {
            x: normalized(x, width),
            y: normalized(y, height),
        }),
        gst_video::NavigationEvent::MouseButtonPress { button, .. } => {
            gst_button(button).map(|button| Event::Button {
                button,
                pressed: true,
            })
        }
        gst_video::NavigationEvent::MouseButtonRelease { button, .. } => {
            gst_button(button).map(|button| Event::Button {
                button,
                pressed: false,
            })
        }
        _ => None,
    }
}

fn gst_button(button: i32) -> Option<u8> {
    match button {
        1 => Some(0),
        2 => Some(2),
        3 => Some(1),
        8 => Some(3),
        9 => Some(4),
        _ => None,
    }
}

fn key_to_windows_vk(key: &str) -> Option<u16> {
    if key.len() == 1 {
        let value = key.as_bytes()[0];
        if value.is_ascii_alphabetic() {
            return Some(u16::from(value.to_ascii_uppercase()));
        }
        if value.is_ascii_digit() {
            return Some(u16::from(value));
        }
    }
    if let Some(number) = key.strip_prefix('F')
        && let Ok(number) = number.parse::<u16>()
        && (1..=12).contains(&number)
    {
        return Some(0x70 + number - 1);
    }
    Some(match key {
        "BackSpace" => 0x08,
        "Tab" => 0x09,
        "Return" | "Enter" => 0x0D,
        "Shift_L" => 0xA0,
        "Shift_R" => 0xA1,
        "Control_L" => 0xA2,
        "Control_R" => 0xA3,
        "Alt_L" => 0xA4,
        "Alt_R" => 0xA5,
        "Escape" => 0x1B,
        "space" | "Space" => 0x20,
        "Left" => 0x25,
        "Up" => 0x26,
        "Right" => 0x27,
        "Down" => 0x28,
        "Home" => 0x24,
        "End" => 0x23,
        "Page_Up" => 0x21,
        "Page_Down" => 0x22,
        "Insert" => 0x2D,
        "Delete" => 0x2E,
        "Super_L" => 0x5B,
        "Super_R" => 0x5C,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_input_uses_the_cross_platform_wire_codes() {
        assert_eq!(key_to_windows_vk("a"), Some(0x41));
        assert_eq!(key_to_windows_vk("Control_R"), Some(0xA3));
        assert_eq!(key_to_windows_vk("F12"), Some(0x7B));
        assert_eq!(gst_button(3), Some(1));
        assert_eq!(normalized(960.0, 1920), 32768);
    }
}
