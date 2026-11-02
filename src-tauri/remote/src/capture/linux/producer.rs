use super::{gst_error, native::Capture};
use crate::Result;
use gst::prelude::*;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub(super) struct Producer {
    cancel: Arc<AtomicBool>,
    failure: Arc<Mutex<Option<String>>>,
    thread: Option<JoinHandle<()>>,
}

impl Producer {
    pub fn start(mut capture: Capture, source: gst_app::AppSrc, fps: u16) -> Result<Self> {
        let cancel = Arc::new(AtomicBool::new(false));
        let failure = Arc::new(Mutex::new(None));
        let worker_cancel = cancel.clone();
        let worker_failure = failure.clone();
        let thread =
            std::thread::Builder::new()
                .name("noosphere-capture".into())
                .spawn(move || {
                    let result =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<()> {
                            let period = Duration::from_nanos(1_000_000_000 / u64::from(fps));
                            let mut dimensions = None;
                            let mut next = Instant::now();
                            while !worker_cancel.load(Ordering::Relaxed) {
                                let captured_at = source.current_running_time();
                                let Some(image) = capture.next(&worker_cancel)? else {
                                    break;
                                };
                                if dimensions != Some((image.width, image.height)) {
                                    source.set_caps(Some(
                                        &gst::Caps::builder("video/x-raw")
                                            .field("format", "BGRx")
                                            .field("width", image.width as i32)
                                            .field("height", image.height as i32)
                                            .field("framerate", gst::Fraction::new(fps as i32, 1))
                                            .build(),
                                    ));
                                    dimensions = Some((image.width, image.height));
                                }
                                let mut buffer = gst::Buffer::from_mut_slice(image.pixels);
                                buffer.get_mut().unwrap().set_pts(captured_at);
                                buffer.get_mut().unwrap().set_duration(
                                    gst::ClockTime::from_nseconds(period.as_nanos() as u64),
                                );
                                source.push_buffer(buffer).map_err(|e| {
                                    gst_error("alimentation de la capture native", e)
                                })?;
                                next += period;
                                let now = Instant::now();
                                if next > now {
                                    std::thread::park_timeout(next - now);
                                } else {
                                    next = now;
                                }
                            }
                            Ok(())
                        }));
                    if !worker_cancel.load(Ordering::Relaxed) {
                        let failure = match result {
                            Ok(Ok(())) => None,
                            Ok(Err(error)) => Some(error.to_string()),
                            Err(_) => Some("Le moteur de capture native s’est arrêté.".into()),
                        };
                        *worker_failure.lock().unwrap_or_else(|e| e.into_inner()) = failure;
                        let _ = source.end_of_stream();
                    }
                })?;
        Ok(Self {
            cancel,
            failure,
            thread: Some(thread),
        })
    }

    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(thread) = &self.thread {
            thread.thread().unpark();
        }
    }

    pub fn failure(&self) -> Option<String> {
        self.failure
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl Drop for Producer {
    fn drop(&mut self) {
        self.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
