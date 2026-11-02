use crate::{Error, Result, capture::Display};
use std::{
    fs::File,
    io::{Read, Seek},
    os::fd::AsFd,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_buffer::{self, WlBuffer},
        wl_output::{self, WlOutput},
        wl_registry::WlRegistry,
        wl_shm::{self, WlShm},
        wl_shm_pool::WlShmPool,
    },
};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1},
    zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
};

mod pixels;

fn error(context: &str, value: impl std::fmt::Display) -> Error {
    Error::Unavailable(format!("Linux · {context} : {value}"))
}

#[derive(Clone)]
struct Output {
    global: u32,
    proxy: WlOutput,
    name: Option<String>,
    width: u32,
    height: u32,
    transform: wl_output::Transform,
}

impl Output {
    fn display(&self, id: u32) -> Display {
        let (width, height) = if matches!(
            self.transform,
            wl_output::Transform::_90
                | wl_output::Transform::_270
                | wl_output::Transform::Flipped90
                | wl_output::Transform::Flipped270
        ) {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        };
        Display {
            id,
            name: self.name.clone().unwrap_or_default(),
            width,
            height,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Layout {
    format: wl_shm::Format,
    width: u32,
    height: u32,
    stride: u32,
}

impl Layout {
    fn length(self) -> Result<usize> {
        let length = u64::from(self.stride) * u64::from(self.height);
        if self.width == 0
            || self.height == 0
            || self.width > 16384
            || self.height > 16384
            || self.stride < self.width.checked_mul(4).ok_or(Error::ResourceLimit)?
            || length > 256 * 1024 * 1024
        {
            return Err(Error::ResourceLimit);
        }
        Ok(length as usize)
    }
}

#[derive(Default)]
struct State {
    outputs: Vec<Output>,
    layout: Option<Layout>,
    ready: bool,
    failed: bool,
    inverted: bool,
    buffer_id: u64,
    released: bool,
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        state: &mut Self,
        _: &WlRegistry,
        event: <WlRegistry as Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wayland_client::protocol::wl_registry::Event::GlobalRemove { name } = event {
            state.outputs.retain(|output| output.global != name);
        }
    }
}

impl Dispatch<WlOutput, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &WlOutput,
        event: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(output) = state
            .outputs
            .iter_mut()
            .find(|output| output.proxy == *proxy)
        else {
            return;
        };
        match event {
            wl_output::Event::Name { name } => output.name = Some(name),
            wl_output::Event::Geometry { transform, .. } => {
                if let Ok(transform) = transform.into_result() {
                    output.transform = transform;
                }
            }
            wl_output::Event::Mode {
                flags,
                width,
                height,
                ..
            } if flags
                .into_result()
                .is_ok_and(|flags| flags.contains(wl_output::Mode::Current))
                && width > 0
                && height > 0 =>
            {
                output.width = width as u32;
                output.height = height as u32;
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwlrScreencopyFrameV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_screencopy_frame_v1::Event::Buffer {
                format,
                width,
                height,
                stride,
            } => match format.into_result() {
                Ok(format) => {
                    state.layout = Some(Layout {
                        format,
                        width,
                        height,
                        stride,
                    })
                }
                Err(_) => state.failed = true,
            },
            zwlr_screencopy_frame_v1::Event::Flags { flags } => {
                state.inverted = flags
                    .into_result()
                    .is_ok_and(|flags| flags.contains(zwlr_screencopy_frame_v1::Flags::YInvert));
            }
            zwlr_screencopy_frame_v1::Event::Ready { .. } => state.ready = true,
            zwlr_screencopy_frame_v1::Event::Failed => state.failed = true,
            _ => {}
        }
    }
}

impl Dispatch<WlBuffer, u64> for State {
    fn event(
        state: &mut Self,
        _: &WlBuffer,
        event: wl_buffer::Event,
        id: &u64,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(event, wl_buffer::Event::Release) && state.buffer_id == *id {
            state.released = true;
        }
    }
}

delegate_noop!(State: ignore WlShm);
delegate_noop!(State: ignore WlShmPool);
delegate_noop!(State: ignore ZwlrScreencopyManagerV1);

struct Context {
    connection: Connection,
    queue: EventQueue<State>,
    state: State,
    manager: ZwlrScreencopyManagerV1,
    shm: WlShm,
}

impl Context {
    fn connect() -> Result<Option<Self>> {
        if std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return Ok(None);
        }
        let connection = Connection::connect_to_env().map_err(|e| error("connexion Wayland", e))?;
        let (globals, mut queue) = registry_queue_init::<State>(&connection)
            .map_err(|e| error("protocoles de capture Wayland", e))?;
        let handle = queue.handle();
        let Ok(manager) = globals.bind::<ZwlrScreencopyManagerV1, _, _>(&handle, 1..=1, ()) else {
            return Ok(None);
        };
        let shm = globals
            .bind::<WlShm, _, _>(&handle, 1..=1, ())
            .map_err(|e| error("mémoire de capture Wayland", e))?;
        let mut state = State::default();
        for global in globals.contents().clone_list() {
            if global.interface == "wl_output" && global.version >= 4 {
                let proxy = globals
                    .registry()
                    .bind::<WlOutput, _, _>(global.name, 4, &handle, ());
                state.outputs.push(Output {
                    global: global.name,
                    proxy,
                    name: None,
                    width: 0,
                    height: 0,
                    transform: wl_output::Transform::Normal,
                });
            }
        }
        queue
            .roundtrip(&mut state)
            .map_err(|e| error("identification des moniteurs", e))?;
        state.outputs.retain(|output| {
            output.name.as_ref().is_some_and(|name| !name.is_empty())
                && output.width > 0
                && output.height > 0
        });
        state.outputs.sort_by_key(|output| {
            let name = output.name.clone().unwrap_or_default();
            (
                !name.starts_with("eDP-") && !name.starts_with("LVDS-"),
                name,
            )
        });
        if state.outputs.len() > 32 {
            return Err(Error::ResourceLimit);
        }
        Ok(Some(Self {
            connection,
            queue,
            state,
            manager,
            shm,
        }))
    }

    fn dispatch(&mut self) -> Result<()> {
        self.queue
            .dispatch_pending(&mut self.state)
            .map_err(|e| error("événements de capture", e))?;
        self.connection
            .flush()
            .map_err(|e| error("requête de capture", e))?;
        if let Some(reader) = self.connection.prepare_read() {
            match reader.read() {
                Ok(_) => {}
                Err(wayland_client::backend::WaylandError::Io(e))
                    if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(error("réception de la capture", e)),
            }
        }
        self.queue
            .dispatch_pending(&mut self.state)
            .map_err(|e| error("événements de capture", e))?;
        Ok(())
    }
}

struct Buffer {
    file: File,
    proxy: WlBuffer,
    pool: WlShmPool,
    layout: Layout,
}

impl Drop for Buffer {
    fn drop(&mut self) {
        self.proxy.destroy();
        self.pool.destroy();
    }
}

struct Frame(ZwlrScreencopyFrameV1);
impl Drop for Frame {
    fn drop(&mut self) {
        self.0.destroy();
    }
}

pub struct Image {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub struct Capture {
    context: Context,
    output: Output,
    display: Display,
    buffer: Option<Buffer>,
}

pub fn displays() -> Result<Option<Vec<Display>>> {
    Ok(Context::connect()?.map(|context| {
        context
            .state
            .outputs
            .iter()
            .enumerate()
            .map(|(id, output)| output.display(id as u32))
            .collect()
    }))
}

impl Capture {
    pub fn open(display: u32, name: Option<&str>) -> Result<Option<Self>> {
        let Some(context) = Context::connect()? else {
            return Ok(None);
        };
        let output = if let Some(name) = name {
            context
                .state
                .outputs
                .iter()
                .find(|output| output.name.as_deref() == Some(name))
        } else {
            context.state.outputs.get(display as usize)
        }
        .cloned()
        .ok_or_else(|| error("écran partagé", "le moniteur sélectionné est déconnecté"))?;
        let display = output.display(display);
        Ok(Some(Self {
            context,
            output,
            display,
            buffer: None,
        }))
    }

    pub fn display(&self) -> &Display {
        &self.display
    }

    pub fn next(&mut self, cancel: &AtomicBool) -> Result<Option<Image>> {
        self.context.state.ready = false;
        self.context.state.failed = false;
        self.context.state.inverted = false;
        self.context.state.layout = None;
        self.context.dispatch()?;
        if !self
            .context
            .state
            .outputs
            .iter()
            .any(|output| output.global == self.output.global)
        {
            return Err(error("écran partagé", "le moniteur a été déconnecté"));
        }
        let frame = Frame(self.context.manager.capture_output(
            1,
            &self.output.proxy,
            &self.context.queue.handle(),
            (),
        ));
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut copied = false;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            self.context.dispatch()?;
            if self.context.state.failed {
                return Err(error(
                    "capture native",
                    "le compositeur a refusé la capture du moniteur",
                ));
            }
            if !copied && let Some(layout) = self.context.state.layout {
                let length = layout.length()?;
                if self
                    .buffer
                    .as_ref()
                    .is_none_or(|buffer| buffer.layout != layout)
                {
                    self.buffer = None;
                    let file = tempfile::tempfile()?;
                    file.set_len(length as u64)?;
                    let pool = self.context.shm.create_pool(
                        file.as_fd(),
                        length as i32,
                        &self.context.queue.handle(),
                        (),
                    );
                    self.context.state.buffer_id += 1;
                    let proxy = pool.create_buffer(
                        0,
                        layout.width as i32,
                        layout.height as i32,
                        layout.stride as i32,
                        layout.format,
                        &self.context.queue.handle(),
                        self.context.state.buffer_id,
                    );
                    self.buffer = Some(Buffer {
                        file,
                        proxy,
                        pool,
                        layout,
                    });
                    self.context.state.released = true;
                }
                if self.context.state.released {
                    self.context.state.released = false;
                    frame.0.copy(&self.buffer.as_ref().unwrap().proxy);
                    copied = true;
                }
            }
            if self.context.state.ready {
                break;
            }
            if Instant::now() >= deadline {
                return Err(error(
                    "capture native",
                    "le moniteur ne fournit plus d’images",
                ));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let buffer = self.buffer.as_mut().ok_or(Error::InvalidPacket)?;
        let mut data = vec![0; buffer.layout.length()?];
        buffer.file.rewind()?;
        buffer.file.read_exact(&mut data)?;
        let transform = self
            .context
            .state
            .outputs
            .iter()
            .find(|output| output.global == self.output.global)
            .map(|output| output.transform)
            .ok_or_else(|| error("écran partagé", "le moniteur a été déconnecté"))?;
        pixels::convert(data, buffer.layout, self.context.state.inverted, transform).map(Some)
    }
}
