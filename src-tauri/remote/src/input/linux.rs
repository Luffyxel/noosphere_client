//! Linux capture authorization and input injection. GNOME/KDE use the
//! RemoteDesktop portal. wlroots compositors (Hyprland/Sway) use their native
//! virtual keyboard/pointer protocols when that portal interface is absent.
use crate::{Error, Result, capture::linux::Source, input::Event, permissions::Permissions};
use ashpd::desktop::{
    PersistMode, Session,
    remote_desktop::{DeviceType, KeyState, RemoteDesktop, SelectDevicesOptions},
    screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType},
};
use std::{
    io::{Seek, Write},
    os::fd::AsFd,
    time::Instant,
};

enum Portal {
    Test,
    X11(Box<X11Input>),
    Remote {
        proxy: RemoteDesktop,
        _screencast: Screencast,
        session: Session<RemoteDesktop>,
    },
    Screencast {
        _proxy: Screencast,
        _session: Session<Screencast>,
        wlr: Option<WlrInput>,
    },
}

pub struct DesktopSession {
    portal: Portal,
    node_id: u32,
    width: u32,
    height: u32,
}

fn portal_error(context: &str, error: impl std::fmt::Display) -> Error {
    Error::Unavailable(format!("Linux · {context} : {error}"))
}

fn requested_devices(permissions: Permissions) -> Option<enumflags2::BitFlags<DeviceType>> {
    match (permissions.keyboard, permissions.mouse) {
        (true, true) => Some(DeviceType::Keyboard | DeviceType::Pointer),
        (true, false) => Some(DeviceType::Keyboard.into()),
        (false, true) => Some(DeviceType::Pointer.into()),
        (false, false) => None,
    }
}

fn portal_supports_requested_devices(
    available: enumflags2::BitFlags<DeviceType>,
    requested: enumflags2::BitFlags<DeviceType>,
) -> bool {
    requested.iter().all(|device| available.contains(device))
}

impl DesktopSession {
    pub async fn open(permissions: Permissions) -> Result<(Self, Source)> {
        if std::env::var_os("NOOSPHERE_REMOTE_TEST_SOURCE").is_some() {
            return Ok((
                Self {
                    portal: Portal::Test,
                    node_id: 0,
                    width: 1920,
                    height: 1080,
                },
                Source::test(),
            ));
        }
        if std::env::var_os("WAYLAND_DISPLAY").is_none()
            && let Ok(display) = std::env::var("DISPLAY")
        {
            let (input, width, height) = X11Input::open()?;
            return Ok((
                Self {
                    portal: Portal::X11(Box::new(input)),
                    node_id: 0,
                    width,
                    height,
                },
                Source::x11(display)?,
            ));
        }
        let screencast = Screencast::new()
            .await
            .map_err(|error| portal_error("portail de partage d’écran indisponible", error))?;

        if let Some(devices) = requested_devices(permissions)
            && let Ok(remote) = RemoteDesktop::new().await
            && remote
                .available_device_types()
                .await
                .is_ok_and(|available| portal_supports_requested_devices(available, devices))
        {
            let session = remote
                .create_session(Default::default())
                .await
                .map_err(|error| portal_error("création de la session distante", error))?;
            remote
                .select_devices(
                    &session,
                    SelectDevicesOptions::default()
                        .set_devices(devices)
                        .set_persist_mode(PersistMode::ExplicitlyRevoked),
                )
                .await
                .map_err(|error| portal_error("autorisation du clavier et de la souris", error))?;
            screencast
                .select_sources(
                    &session,
                    SelectSourcesOptions::default()
                        .set_cursor_mode(CursorMode::Embedded)
                        .set_sources(enumflags2::BitFlags::from(SourceType::Monitor))
                        .set_multiple(false)
                        .set_persist_mode(PersistMode::ExplicitlyRevoked),
                )
                .await
                .map_err(|error| portal_error("sélection de l’écran", error))?;
            let response = remote
                .start(&session, None, Default::default())
                .await
                .map_err(|error| portal_error("ouverture de l’autorisation système", error))?
                .response()
                .map_err(|error| portal_error("autorisation système refusée", error))?;
            let stream = response
                .streams()
                .first()
                .ok_or_else(|| portal_error("capture", "aucun écran sélectionné"))?;
            let node_id = stream.pipe_wire_node_id();
            let (width, height) = dimensions(stream.size());
            let fd = screencast
                .open_pipe_wire_remote(&session, Default::default())
                .await
                .map_err(|error| portal_error("connexion PipeWire", error))?;
            return Ok((
                Self {
                    portal: Portal::Remote {
                        proxy: remote,
                        _screencast: screencast,
                        session,
                    },
                    node_id,
                    width,
                    height,
                },
                Source {
                    fd: Some(fd),
                    node_id: Some(node_id),
                    x11_display: None,
                },
            ));
        }

        let session = screencast
            .create_session(Default::default())
            .await
            .map_err(|error| portal_error("création de la capture", error))?;
        screencast
            .select_sources(
                &session,
                SelectSourcesOptions::default()
                    .set_cursor_mode(CursorMode::Embedded)
                    .set_sources(enumflags2::BitFlags::from(SourceType::Monitor))
                    .set_multiple(false)
                    .set_persist_mode(PersistMode::ExplicitlyRevoked),
            )
            .await
            .map_err(|error| portal_error("sélection de l’écran", error))?;
        let response = screencast
            .start(&session, None, Default::default())
            .await
            .map_err(|error| portal_error("ouverture de l’autorisation système", error))?
            .response()
            .map_err(|error| portal_error("autorisation système refusée", error))?;
        let stream = response
            .streams()
            .first()
            .ok_or_else(|| portal_error("capture", "aucun écran sélectionné"))?;
        let node_id = stream.pipe_wire_node_id();
        let (width, height) = dimensions(stream.size());
        let fd = screencast
            .open_pipe_wire_remote(&session, Default::default())
            .await
            .map_err(|error| portal_error("connexion PipeWire", error))?;
        let wlr = if requested_devices(permissions).is_some() {
            Some(WlrInput::open()?)
        } else {
            None
        };
        Ok((
            Self {
                portal: Portal::Screencast {
                    _proxy: screencast,
                    _session: session,
                    wlr,
                },
                node_id,
                width,
                height,
            },
            Source {
                fd: Some(fd),
                node_id: Some(node_id),
                x11_display: None,
            },
        ))
    }

    pub async fn inject(&mut self, event: &Event) -> Result<()> {
        match &mut self.portal {
            Portal::Test => Ok(()),
            Portal::X11(input) => input.inject(event),
            Portal::Remote { proxy, session, .. } => {
                inject_portal(proxy, session, self.node_id, self.width, self.height, event).await
            }
            Portal::Screencast { wlr, .. } => match wlr {
                Some(wlr) => wlr.inject(event),
                None => Ok(()),
            },
        }
    }
}

struct X11Input {
    connection: x11rb::rust_connection::RustConnection,
    root: x11rb::protocol::xproto::Window,
    width: u32,
    height: u32,
}

impl X11Input {
    fn open() -> Result<(Self, u32, u32)> {
        use x11rb::connection::Connection as _;

        let (connection, screen_index) =
            x11rb::connect(None).map_err(|error| portal_error("connexion X11", error))?;
        let screen = connection
            .setup()
            .roots
            .get(screen_index)
            .ok_or_else(|| portal_error("connexion X11", "écran absent"))?;
        let root = screen.root;
        let width = u32::from(screen.width_in_pixels);
        let height = u32::from(screen.height_in_pixels);
        Ok((
            Self {
                connection,
                root,
                width,
                height,
            },
            width,
            height,
        ))
    }

    fn inject(&self, event: &Event) -> Result<()> {
        use x11rb::{
            connection::Connection as _,
            protocol::{xproto, xtest::ConnectionExt as _},
        };
        let request = match *event {
            Event::Key { code, pressed } => {
                let evdev = windows_vk_to_evdev(code).ok_or(Error::InvalidPacket)?;
                let keycode = u8::try_from(evdev + 8).map_err(|_| Error::InvalidPacket)?;
                self.connection.xtest_fake_input(
                    if pressed {
                        xproto::KEY_PRESS_EVENT
                    } else {
                        xproto::KEY_RELEASE_EVENT
                    },
                    keycode,
                    0,
                    self.root,
                    0,
                    0,
                    0,
                )
            }
            Event::Pointer { x, y } => self.connection.xtest_fake_input(
                xproto::MOTION_NOTIFY_EVENT,
                0,
                0,
                self.root,
                scale_pointer(x, self.width),
                scale_pointer(y, self.height),
                0,
            ),
            Event::Button { button, pressed } => self.connection.xtest_fake_input(
                if pressed {
                    xproto::BUTTON_PRESS_EVENT
                } else {
                    xproto::BUTTON_RELEASE_EVENT
                },
                x11_button(button)?,
                0,
                self.root,
                0,
                0,
                0,
            ),
            Event::Gamepad { .. } | Event::Clipboard(_) => return Ok(()),
        };
        request
            .map_err(|error| portal_error("entrée X11", error))?
            .check()
            .map_err(|error| portal_error("entrée X11", error))?;
        self.connection
            .flush()
            .map_err(|error| portal_error("entrée X11", error))?;
        Ok(())
    }
}

fn scale_pointer(value: u16, dimension: u32) -> i16 {
    let coordinate =
        u32::from(value).saturating_mul(dimension.saturating_sub(1)) / u32::from(u16::MAX);
    coordinate.min(i16::MAX as u32) as i16
}

fn x11_button(button: u8) -> Result<u8> {
    [1, 3, 2, 8, 9]
        .get(button as usize)
        .copied()
        .ok_or(Error::InvalidPacket)
}

fn dimensions(size: Option<(i32, i32)>) -> (u32, u32) {
    size.filter(|(width, height)| *width > 0 && *height > 0)
        .map(|(width, height)| (width as u32, height as u32))
        .unwrap_or((1920, 1080))
}

async fn inject_portal(
    proxy: &RemoteDesktop,
    session: &Session<RemoteDesktop>,
    node_id: u32,
    width: u32,
    height: u32,
    event: &Event,
) -> Result<()> {
    match *event {
        Event::Key { code, pressed } => {
            let code = windows_vk_to_evdev(code)
                .ok_or_else(|| portal_error("clavier", format!("touche non reconnue : {code}")))?;
            proxy
                .notify_keyboard_keycode(
                    session,
                    code as i32,
                    if pressed {
                        KeyState::Pressed
                    } else {
                        KeyState::Released
                    },
                    Default::default(),
                )
                .await
                .map_err(|error| portal_error("injection clavier", error))
        }
        Event::Pointer { x, y } => proxy
            .notify_pointer_motion_absolute(
                session,
                node_id,
                f64::from(x) * f64::from(width) / f64::from(u16::MAX),
                f64::from(y) * f64::from(height) / f64::from(u16::MAX),
                Default::default(),
            )
            .await
            .map_err(|error| portal_error("injection souris", error)),
        Event::Button { button, pressed } => proxy
            .notify_pointer_button(
                session,
                linux_button(button)? as i32,
                if pressed {
                    KeyState::Pressed
                } else {
                    KeyState::Released
                },
                Default::default(),
            )
            .await
            .map_err(|error| portal_error("bouton de souris", error)),
        Event::Gamepad { .. } | Event::Clipboard(_) => Ok(()),
    }
}

fn linux_button(button: u8) -> Result<u32> {
    [272, 273, 274, 275, 276]
        .get(button as usize)
        .copied()
        .ok_or(Error::InvalidPacket)
}

fn windows_vk_to_evdev(code: u16) -> Option<u32> {
    let mapped = match code {
        0x08 => 14,
        0x09 => 15,
        0x0D => 28,
        0x10 | 0xA0 => 42,
        0xA1 => 54,
        0x11 | 0xA2 => 29,
        0xA3 => 97,
        0x12 | 0xA4 => 56,
        0xA5 => 100,
        0x14 => 58,
        0x1B => 1,
        0x20 => 57,
        0x21 => 104,
        0x22 => 109,
        0x23 => 107,
        0x24 => 102,
        0x25 => 105,
        0x26 => 103,
        0x27 => 106,
        0x28 => 108,
        0x2D => 110,
        0x2E => 111,
        0x30..=0x39 => [11, 2, 3, 4, 5, 6, 7, 8, 9, 10][(code - 0x30) as usize],
        0x41..=0x5A => [
            30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47,
            17, 45, 21, 44,
        ][(code - 0x41) as usize],
        0x5B => 125,
        0x5C => 126,
        0x60..=0x69 => [82, 79, 80, 81, 75, 76, 77, 71, 72, 73][(code - 0x60) as usize],
        0x6A => 55,
        0x6B => 78,
        0x6D => 74,
        0x6E => 83,
        0x6F => 98,
        0x70..=0x7B => (59 + code - 0x70) as u32,
        0x90 => 69,
        0x91 => 70,
        0xBA => 39,
        0xBB => 13,
        0xBC => 51,
        0xBD => 12,
        0xBE => 52,
        0xBF => 53,
        0xC0 => 41,
        0xDB => 26,
        0xDC => 43,
        0xDD => 27,
        0xDE => 40,
        _ => return None,
    };
    Some(mapped)
}

struct WaylandState;

use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{wl_pointer, wl_registry::WlRegistry, wl_seat::WlSeat},
};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

impl Dispatch<WlRegistry, GlobalListContents> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
delegate_noop!(WaylandState: ignore WlSeat);
delegate_noop!(WaylandState: ignore ZwpVirtualKeyboardManagerV1);
delegate_noop!(WaylandState: ignore ZwpVirtualKeyboardV1);
delegate_noop!(WaylandState: ignore ZwlrVirtualPointerManagerV1);
delegate_noop!(WaylandState: ignore ZwlrVirtualPointerV1);

struct WlrInput {
    connection: Connection,
    queue: EventQueue<WaylandState>,
    state: WaylandState,
    keyboard: ZwpVirtualKeyboardV1,
    pointer: ZwlrVirtualPointerV1,
    _keymap: std::fs::File,
    started: Instant,
}

pub fn probe_wlr_input() -> Result<()> {
    let _ = WlrInput::open()?;
    Ok(())
}

/// Drives the compositor-native pointer during an interactive Linux probe.
/// Coordinates use the same normalized range as remote input packets.
#[doc(hidden)]
pub fn probe_wlr_click(x: u16, y: u16) -> Result<()> {
    let mut input = WlrInput::open()?;
    input.inject(&Event::Pointer { x, y })?;
    for click in 0..2 {
        input.inject(&Event::Button {
            button: 0,
            pressed: true,
        })?;
        std::thread::sleep(std::time::Duration::from_millis(40));
        input.inject(&Event::Button {
            button: 0,
            pressed: false,
        })?;
        if click == 0 {
            std::thread::sleep(std::time::Duration::from_millis(90));
        }
    }
    Ok(())
}

/// Activates the selected widget with the compositor-native virtual keyboard.
#[doc(hidden)]
pub fn probe_wlr_activate() -> Result<()> {
    let mut input = WlrInput::open()?;
    for code in [0x09, 0x0D] {
        input.inject(&Event::Key {
            code,
            pressed: true,
        })?;
        std::thread::sleep(std::time::Duration::from_millis(40));
        input.inject(&Event::Key {
            code,
            pressed: false,
        })?;
        std::thread::sleep(std::time::Duration::from_millis(80));
    }
    Ok(())
}

impl WlrInput {
    fn open() -> Result<Self> {
        let connection = Connection::connect_to_env()
            .map_err(|error| portal_error("connexion au compositeur Wayland", error))?;
        let (globals, mut queue) = registry_queue_init::<WaylandState>(&connection)
            .map_err(|error| portal_error("protocoles Wayland", error))?;
        let handle = queue.handle();
        let seat: WlSeat = globals
            .bind(&handle, 1..=9, ())
            .map_err(|error| portal_error("siège Wayland", error))?;
        let keyboard_manager: ZwpVirtualKeyboardManagerV1 =
            globals.bind(&handle, 1..=1, ()).map_err(|_| {
                portal_error(
                    "clavier distant",
                    "ce compositeur ne fournit ni RemoteDesktop portal ni virtual-keyboard",
                )
            })?;
        let pointer_manager: ZwlrVirtualPointerManagerV1 =
            globals.bind(&handle, 1..=2, ()).map_err(|_| {
                portal_error(
                    "souris distante",
                    "ce compositeur ne fournit ni RemoteDesktop portal ni wlr-virtual-pointer",
                )
            })?;
        let keyboard = keyboard_manager.create_virtual_keyboard(&seat, &handle, ());
        let pointer = pointer_manager.create_virtual_pointer(Some(&seat), &handle, ());

        let context = xkbcommon::xkb::Context::new(xkbcommon::xkb::CONTEXT_NO_FLAGS);
        let keymap = xkbcommon::xkb::Keymap::new_from_names(
            &context,
            "",
            "",
            "",
            "",
            None,
            xkbcommon::xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .ok_or_else(|| portal_error("clavier distant", "impossible de créer la keymap"))?;
        let keymap_text = keymap.get_as_string(xkbcommon::xkb::KEYMAP_FORMAT_TEXT_V1);
        let mut keymap_file =
            tempfile::tempfile().map_err(|error| portal_error("fichier de keymap", error))?;
        keymap_file
            .write_all(keymap_text.as_bytes())
            .and_then(|_| keymap_file.flush())
            .and_then(|_| keymap_file.rewind())
            .map_err(|error| portal_error("écriture de la keymap", error))?;
        keyboard.keymap(1, keymap_file.as_fd(), keymap_text.len() as u32);
        connection
            .flush()
            .map_err(|error| portal_error("initialisation des entrées Wayland", error))?;
        let mut state = WaylandState;
        queue
            .roundtrip(&mut state)
            .map_err(|error| portal_error("initialisation des entrées Wayland", error))?;
        Ok(Self {
            connection,
            queue,
            state,
            keyboard,
            pointer,
            _keymap: keymap_file,
            started: Instant::now(),
        })
    }

    fn inject(&mut self, event: &Event) -> Result<()> {
        let time = self.started.elapsed().as_millis() as u32;
        match *event {
            Event::Key { code, pressed } => self.keyboard.key(
                time,
                windows_vk_to_evdev(code).ok_or(Error::InvalidPacket)?,
                u32::from(pressed),
            ),
            Event::Pointer { x, y } => self.pointer.motion_absolute(
                time,
                u32::from(x),
                u32::from(y),
                u32::from(u16::MAX),
                u32::from(u16::MAX),
            ),
            Event::Button { button, pressed } => self.pointer.button(
                time,
                linux_button(button)?,
                if pressed {
                    wl_pointer::ButtonState::Pressed
                } else {
                    wl_pointer::ButtonState::Released
                },
            ),
            Event::Gamepad { .. } | Event::Clipboard(_) => return Ok(()),
        }
        self.pointer.frame();
        self.connection
            .flush()
            .map_err(|error| portal_error("envoi des entrées Wayland", error))?;
        self.queue
            .dispatch_pending(&mut self.state)
            .map_err(|error| portal_error("réponse des entrées Wayland", error))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_cross_platform_keys_and_buttons_to_linux_input() {
        assert_eq!(windows_vk_to_evdev(0x41), Some(30));
        assert_eq!(windows_vk_to_evdev(0x0D), Some(28));
        assert_eq!(windows_vk_to_evdev(0x70), Some(59));
        assert_eq!(linux_button(0).unwrap(), 272);
        assert_eq!(x11_button(0).unwrap(), 1);
        assert_eq!(x11_button(1).unwrap(), 3);
        assert_eq!(scale_pointer(u16::MAX, 1920), 1919);
        assert!(windows_vk_to_evdev(0xFF).is_none());
    }

    #[test]
    fn standard_remote_desktop_portals_cover_gnome_and_kde_permissions() {
        let requested = requested_devices(Permissions {
            screen: true,
            keyboard: true,
            mouse: true,
            ..Permissions::default()
        })
        .unwrap();
        assert!(portal_supports_requested_devices(
            DeviceType::Keyboard | DeviceType::Pointer,
            requested
        ));
        assert!(!portal_supports_requested_devices(
            DeviceType::Keyboard.into(),
            requested
        ));
        assert!(
            requested_devices(Permissions {
                screen: true,
                ..Permissions::default()
            })
            .is_none()
        );
    }
}
