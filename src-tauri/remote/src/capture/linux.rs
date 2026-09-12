//! Linux media path. The compositor grants a PipeWire node through the XDG
//! portal; GStreamer keeps the live queue at one frame and emits one H.264
//! access unit per sample.
use crate::{
    Error, Result,
    encoder::{Codec, VideoSettings},
};
use bytes::Bytes;
use gst::prelude::*;
use std::os::fd::OwnedFd;

pub struct Source {
    pub fd: Option<OwnedFd>,
    pub node_id: Option<u32>,
    pub x11_display: Option<String>,
}

impl Source {
    pub fn test() -> Self {
        Self {
            fd: None,
            node_id: None,
            x11_display: None,
        }
    }

    pub fn x11(display: String) -> Result<Self> {
        if display.is_empty()
            || display.len() > 128
            || !display
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b":._-/".contains(&byte))
        {
            return Err(Error::InvalidPacket);
        }
        Ok(Self {
            fd: None,
            node_id: None,
            x11_display: Some(display),
        })
    }
}

#[derive(Clone, Debug)]
pub struct Probe {
    pub ready: bool,
    pub encoder: Option<String>,
    pub hardware: bool,
    pub unavailable: Vec<String>,
}

#[derive(Clone, Copy)]
struct EncoderChoice {
    factory: &'static str,
    description: &'static str,
    hardware: bool,
}

const HARDWARE_ENCODERS: [EncoderChoice; 5] = [
    EncoderChoice {
        factory: "nvh264enc",
        description: "NVIDIA NVENC H.264",
        hardware: true,
    },
    EncoderChoice {
        factory: "vah264lpenc",
        description: "VA-API H.264 Low Power",
        hardware: true,
    },
    EncoderChoice {
        factory: "vah264enc",
        description: "VA-API H.264",
        hardware: true,
    },
    EncoderChoice {
        factory: "vaapih264enc",
        description: "VA-API H.264 (GStreamer 1.20)",
        hardware: true,
    },
    EncoderChoice {
        factory: "qsvh264enc",
        description: "Intel Quick Sync H.264",
        hardware: true,
    },
];

const SOFTWARE_ENCODERS: [EncoderChoice; 2] = [
    EncoderChoice {
        factory: "x264enc",
        description: "x264 H.264 (test/compatibilité)",
        hardware: false,
    },
    EncoderChoice {
        factory: "openh264enc",
        description: "OpenH264 (test/compatibilité)",
        hardware: false,
    },
];

fn gst_error(context: &str, error: impl std::fmt::Display) -> Error {
    Error::Unavailable(format!("Linux · {context} : {error}"))
}

fn factory_available(name: &str) -> bool {
    gst::ElementFactory::find(name).is_some()
}

fn choose_encoder(allow_software: bool) -> Option<EncoderChoice> {
    HARDWARE_ENCODERS
        .into_iter()
        .chain(
            allow_software
                .then_some(SOFTWARE_ENCODERS)
                .into_iter()
                .flatten(),
        )
        .find(|choice| factory_available(choice.factory))
}

pub fn probe() -> Probe {
    let mut unavailable = Vec::new();
    if let Err(error) = gst::init() {
        return Probe {
            ready: false,
            encoder: None,
            hardware: false,
            unavailable: vec![format!("GStreamer ne démarre pas : {error}")],
        };
    }
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let x11 = std::env::var_os("DISPLAY").is_some();
    if !wayland && !x11 {
        unavailable.push("Aucune session graphique Linux active.".into());
    }
    if wayland && !factory_available("pipewiresrc") {
        unavailable.push("Le module GStreamer PipeWire est absent.".into());
    }
    if wayland && std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
        unavailable.push("Le bus de la session graphique est absent.".into());
    }
    let allow_software = std::env::var_os("NOOSPHERE_REMOTE_ALLOW_SOFTWARE").is_some()
        || std::env::var_os("NOOSPHERE_REMOTE_TEST_SOURCE").is_some();
    let encoder = choose_encoder(allow_software);
    if encoder.is_none() {
        unavailable
            .push("Aucun encodeur H.264 GPU (NVENC, VA-API ou QSV) n’est disponible.".into());
    }
    Probe {
        ready: unavailable.is_empty(),
        encoder: encoder.map(|encoder| encoder.description.into()),
        hardware: encoder.is_some_and(|encoder| encoder.hardware),
        unavailable,
    }
}

pub struct EncodedFrame {
    pub bytes: Bytes,
    pub keyframe: bool,
}

pub struct PortalEncoder {
    pipeline: gst::Pipeline,
    appsink: gst_app::AppSink,
    encoder: gst::Element,
    _source_fd: Option<OwnedFd>,
    choice: EncoderChoice,
}

impl PortalEncoder {
    pub fn open(source: Source, settings: &VideoSettings) -> Result<Self> {
        settings.validate()?;
        if settings.codec != Codec::H264 {
            return Err(Error::Unavailable(
                "Linux négocie H.264 pour cette version.".into(),
            ));
        }
        gst::init().map_err(|error| gst_error("initialisation GStreamer", error))?;
        let test_source =
            source.fd.is_none() && source.node_id.is_none() && source.x11_display.is_none();
        let choice = choose_encoder(
            test_source || std::env::var_os("NOOSPHERE_REMOTE_ALLOW_SOFTWARE").is_some(),
        )
        .ok_or_else(|| {
            Error::Unavailable("Aucun encodeur H.264 GPU compatible n’est disponible.".into())
        })?;
        let source_description = match (&source.fd, source.node_id, &source.x11_display) {
            (Some(fd), Some(node_id), None) => format!(
                "pipewiresrc fd={} path={} do-timestamp=true",
                std::os::fd::AsRawFd::as_raw_fd(fd),
                node_id
            ),
            (None, None, Some(display)) => format!(
                "ximagesrc display-name={display} use-damage=true show-pointer=true do-timestamp=true"
            ),
            (None, None, None) => "videotestsrc is-live=true do-timestamp=true pattern=ball".into(),
            _ => return Err(Error::InvalidPacket),
        };
        let bitrate_kbps = (settings.bitrate / 1_000).clamp(500, 200_000);
        let encoder_description = match choice.factory {
            "nvh264enc" => format!(
                "nvh264enc name=noosphere_encoder bitrate={bitrate_kbps} gop-size={} bframes=0 zerolatency=true",
                settings.fps
            ),
            "vah264lpenc" | "vah264enc" | "qsvh264enc" => format!(
                "{} name=noosphere_encoder bitrate={bitrate_kbps} key-int-max={}",
                choice.factory, settings.fps
            ),
            "vaapih264enc" => format!(
                "vaapih264enc name=noosphere_encoder bitrate={bitrate_kbps} keyframe-period={} max-bframes=0",
                settings.fps
            ),
            "x264enc" => format!(
                "x264enc name=noosphere_encoder bitrate={bitrate_kbps} key-int-max={} bframes=0 byte-stream=true tune=zerolatency speed-preset=ultrafast sliced-threads=true",
                settings.fps
            ),
            "openh264enc" => format!(
                "openh264enc name=noosphere_encoder bitrate={} gop-size={} complexity=low",
                settings.bitrate, settings.fps
            ),
            _ => return Err(Error::InvalidPacket),
        };
        let raw_format = if choice.factory == "openh264enc" {
            "I420"
        } else {
            "NV12"
        };
        let description = format!(
            "{source_description} ! queue leaky=downstream max-size-buffers=1 max-size-bytes=0 max-size-time=0 ! videoconvert n-threads=4 ! videoscale ! videorate skip-to-first=true ! video/x-raw,format={raw_format},width={},height={},framerate={}/1 ! {encoder_description} ! h264parse config-interval=-1 ! video/x-h264,stream-format=byte-stream,alignment=au ! appsink name=noosphere_sink sync=false max-buffers=1 drop=true enable-last-sample=false",
            settings.width, settings.height, settings.fps
        );
        let pipeline = gst::parse::launch(&description)
            .map_err(|error| gst_error("création de la chaîne d’encodage", error))?
            .downcast::<gst::Pipeline>()
            .map_err(|_| gst_error("création de la chaîne d’encodage", "pipeline invalide"))?;
        let appsink = pipeline
            .by_name("noosphere_sink")
            .ok_or_else(|| gst_error("encodage", "sortie vidéo absente"))?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| gst_error("encodage", "sortie vidéo invalide"))?;
        let encoder = pipeline
            .by_name("noosphere_encoder")
            .ok_or_else(|| gst_error("encodage", "encodeur absent"))?;
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|error| gst_error("démarrage de l’encodeur", error))?;
        Ok(Self {
            pipeline,
            appsink,
            encoder,
            _source_fd: source.fd,
            choice,
        })
    }

    pub fn encoder_name(&self) -> &'static str {
        self.choice.description
    }

    pub fn acquire(&self, timeout_ms: u64) -> Result<Option<EncodedFrame>> {
        if let Some(bus) = self.pipeline.bus()
            && let Some(message) = bus.timed_pop_filtered(
                gst::ClockTime::ZERO,
                &[gst::MessageType::Error, gst::MessageType::Eos],
            )
        {
            return match message.view() {
                gst::MessageView::Error(error) => Err(gst_error(
                    "capture/encodage",
                    format!(
                        "{} ({})",
                        error.error(),
                        error.debug().unwrap_or_else(|| "sans détail".into())
                    ),
                )),
                _ => Err(gst_error("capture/encodage", "flux terminé")),
            };
        }
        let Some(sample) = self
            .appsink
            .try_pull_sample(gst::ClockTime::from_mseconds(timeout_ms))
        else {
            return Ok(None);
        };
        let buffer = sample
            .buffer()
            .ok_or_else(|| gst_error("lecture de l’image encodée", "tampon absent"))?;
        let keyframe = !buffer.flags().contains(gst::BufferFlags::DELTA_UNIT);
        let map = buffer
            .map_readable()
            .map_err(|error| gst_error("lecture du tampon encodé", error))?;
        Ok(Some(EncodedFrame {
            bytes: Bytes::copy_from_slice(map.as_slice()),
            keyframe,
        }))
    }

    pub fn force_keyframe(&self) {
        let event = gst_video::UpstreamForceKeyUnitEvent::builder()
            .all_headers(true)
            .build();
        let _ = self.encoder.send_event(event);
    }

    pub fn set_bitrate(&self, bits_per_second: u64) -> Result<()> {
        let value = if self.choice.factory == "openh264enc" {
            bits_per_second.clamp(500_000, 200_000_000)
        } else {
            (bits_per_second / 1_000).clamp(500, 200_000)
        };
        let property = self
            .encoder
            .find_property("bitrate")
            .ok_or_else(|| gst_error("adaptation du débit", "propriété bitrate absente"))?;
        if property.value_type() == u32::static_type() {
            self.encoder.set_property("bitrate", value as u32);
        } else if property.value_type() == u64::static_type() {
            self.encoder.set_property("bitrate", value);
        } else {
            return Err(gst_error(
                "adaptation du débit",
                "type bitrate non pris en charge",
            ));
        }
        Ok(())
    }
}

impl Drop for PortalEncoder {
    fn drop(&mut self) {
        let _ = self.pipeline.send_event(gst::event::Eos::new());
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}
