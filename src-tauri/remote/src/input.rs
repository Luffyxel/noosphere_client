use crate::{Error, Result, permissions::Permissions, signaling::ReplayWindow};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(windows)]
pub mod windows;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Event {
    Key { code: u16, pressed: bool },
    Pointer { x: u16, y: u16 },
    Button { button: u8, pressed: bool },
    Gamepad { buttons: u32 },
    Clipboard(String),
}

pub struct InputGuard {
    permissions: Permissions,
    replay: ReplayWindow,
    keys: BTreeSet<u16>,
    buttons: BTreeSet<u8>,
    last_state: BTreeMap<(u8, u16), u64>,
}
impl InputGuard {
    /// Full state snapshots repair lost releases without retransmitting stale events.
    pub fn reconcile(&mut self, sequence: u64, held: &[u16], buttons: u8) -> Result<Vec<Event>> {
        if held.len() > 256 || held.iter().any(|&key| key >= 256) || buttons & !31 != 0 {
            return Err(Error::InvalidPacket);
        }
        if (!held.is_empty() && !self.permissions.keyboard)
            || (buttons != 0 && !self.permissions.mouse)
        {
            return Err(Error::PermissionDenied);
        }
        // A snapshot is globally ordered relative to all prior input states.
        if self.last_state.values().any(|&last| last >= sequence) || !self.replay.accept(sequence) {
            return Err(Error::InvalidPacket);
        }
        let next: BTreeSet<_> = held.iter().copied().collect();
        let mut events = Vec::new();
        for code in 0..256 {
            let pressed = next.contains(&code);
            if self.keys.contains(&code) != pressed {
                events.push(Event::Key { code, pressed });
            }
            self.last_state.insert((0, code), sequence);
        }
        self.keys = next;
        for button in 0..5 {
            let pressed = buttons & (1 << button) != 0;
            if self.buttons.contains(&button) != pressed {
                events.push(Event::Button { button, pressed });
            }
            if pressed {
                self.buttons.insert(button);
            } else {
                self.buttons.remove(&button);
            }
            self.last_state.insert((1, button as u16), sequence);
        }
        Ok(events)
    }

    pub fn release_all(&mut self) -> Vec<Event> {
        let permissions = self.permissions;
        let events = self.set_permissions(Permissions::default());
        self.permissions = permissions;
        events
    }
    pub fn new(permissions: Permissions) -> Self {
        Self {
            permissions,
            replay: ReplayWindow::default(),
            keys: BTreeSet::new(),
            buttons: BTreeSet::new(),
            last_state: BTreeMap::new(),
        }
    }
    pub fn accept(&mut self, sequence: u64, event: Event) -> Result<Event> {
        let allowed = match &event {
            Event::Key { code, .. } => self.permissions.keyboard && *code < 256,
            Event::Pointer { .. } => self.permissions.mouse,
            Event::Button { button, .. } => self.permissions.mouse && *button < 5,
            Event::Gamepad { .. } => self.permissions.gamepad,
            Event::Clipboard(text) => self.permissions.clipboard && text.len() <= 64 * 1024,
        };
        if !allowed {
            return Err(Error::PermissionDenied);
        }
        if !self.replay.accept(sequence) {
            return Err(Error::InvalidPacket);
        }
        let state_key = match &event {
            Event::Key { code, .. } => (0, *code),
            Event::Button { button, .. } => (1, u16::from(*button)),
            Event::Pointer { .. } => (2, 0),
            Event::Gamepad { .. } => (3, 0),
            Event::Clipboard(_) => (4, 0),
        };
        if self
            .last_state
            .get(&state_key)
            .is_some_and(|last| sequence <= *last)
        {
            return Err(Error::InvalidPacket);
        }
        self.last_state.insert(state_key, sequence);
        match event {
            Event::Key { code, pressed } => {
                if pressed {
                    self.keys.insert(code);
                } else {
                    self.keys.remove(&code);
                }
            }
            Event::Button { button, pressed } => {
                if pressed {
                    self.buttons.insert(button);
                } else {
                    self.buttons.remove(&button);
                }
            }
            _ => {}
        }
        Ok(event)
    }
    pub fn set_permissions(&mut self, permissions: Permissions) -> Vec<Event> {
        self.permissions = permissions;
        let mut releases = Vec::new();
        if !permissions.keyboard {
            releases.extend(
                std::mem::take(&mut self.keys)
                    .into_iter()
                    .map(|code| Event::Key {
                        code,
                        pressed: false,
                    }),
            );
        }
        if !permissions.mouse {
            releases.extend(std::mem::take(&mut self.buttons).into_iter().map(|button| {
                Event::Button {
                    button,
                    pressed: false,
                }
            }));
        }
        if !permissions.gamepad {
            releases.push(Event::Gamepad { buttons: 0 });
        }
        releases
    }
}

pub trait InputSink {
    fn inject(&mut self, event: &Event) -> Result<()>;
}

/// Owns injected state. Call tick from the host loop even when no datagram arrives.
pub struct InputController<S: InputSink> {
    guard: InputGuard,
    sink: S,
    last_received_us: Option<u64>,
}
impl<S: InputSink> InputController<S> {
    pub fn new(permissions: Permissions, sink: S) -> Self {
        Self {
            guard: InputGuard::new(permissions),
            sink,
            last_received_us: None,
        }
    }
    pub fn event(&mut self, sequence: u64, event: Event, now_us: u64) -> Result<()> {
        let event = self.guard.accept(sequence, event)?;
        self.sink.inject(&event)?;
        self.last_received_us = Some(now_us);
        Ok(())
    }
    pub fn snapshot(
        &mut self,
        sequence: u64,
        keys: &[u16],
        buttons: u8,
        now_us: u64,
    ) -> Result<()> {
        for event in self.guard.reconcile(sequence, keys, buttons)? {
            self.sink.inject(&event)?;
        }
        self.last_received_us = Some(now_us);
        Ok(())
    }
    pub fn set_permissions(&mut self, permissions: Permissions) -> Result<()> {
        let mut error = None;
        for event in self.guard.set_permissions(permissions) {
            if let Err(e) = self.sink.inject(&event) {
                error = Some(e);
            }
        }
        error.map_or(Ok(()), Err)
    }
    pub fn tick(&mut self, now_us: u64) -> Result<()> {
        if self
            .last_received_us
            .is_some_and(|last| now_us.saturating_sub(last) >= 250_000)
        {
            self.last_received_us = None;
            for event in self.guard.release_all() {
                self.sink.inject(&event)?;
            }
        }
        Ok(())
    }
}
impl<S: InputSink> Drop for InputController<S> {
    fn drop(&mut self) {
        for event in self.guard.release_all() {
            let _ = self.sink.inject(&event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshots_repair_lost_releases_and_reject_reordered_presses() {
        let mut guard = InputGuard::new(Permissions {
            keyboard: true,
            mouse: true,
            ..Permissions::default()
        });
        guard
            .accept(
                1,
                Event::Key {
                    code: 65,
                    pressed: true,
                },
            )
            .unwrap();
        guard
            .accept(
                2,
                Event::Button {
                    button: 0,
                    pressed: true,
                },
            )
            .unwrap();
        let events = guard.reconcile(4, &[], 0).unwrap();
        assert!(events.contains(&Event::Key {
            code: 65,
            pressed: false
        }));
        assert!(events.contains(&Event::Button {
            button: 0,
            pressed: false
        }));
        assert!(
            guard
                .accept(
                    3,
                    Event::Key {
                        code: 66,
                        pressed: true
                    }
                )
                .is_err()
        );
        assert!(guard.reconcile(5, &[300], 0).is_err());
    }
    #[test]
    fn watchdog_and_disconnect_release_injected_state() {
        use std::{cell::RefCell, rc::Rc};
        struct Sink(Rc<RefCell<Vec<Event>>>);
        impl InputSink for Sink {
            fn inject(&mut self, event: &Event) -> Result<()> {
                self.0.borrow_mut().push(event.clone());
                Ok(())
            }
        }
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut controller = InputController::new(
            Permissions {
                keyboard: true,
                ..Permissions::default()
            },
            Sink(events.clone()),
        );
        controller
            .event(
                1,
                Event::Key {
                    code: 65,
                    pressed: true,
                },
                1,
            )
            .unwrap();
        controller.tick(250_001).unwrap();
        assert!(events.borrow().contains(&Event::Key {
            code: 65,
            pressed: false
        }));
        controller
            .event(
                2,
                Event::Key {
                    code: 66,
                    pressed: true,
                },
                300_000,
            )
            .unwrap();
        drop(controller);
        assert!(events.borrow().contains(&Event::Key {
            code: 66,
            pressed: false
        }));
    }
    #[test]
    fn reordered_key_down_does_not_resurrect_a_released_key() {
        let mut input = InputGuard::new(Permissions {
            keyboard: true,
            ..Permissions::default()
        });
        input
            .accept(
                2,
                Event::Key {
                    code: 42,
                    pressed: false,
                },
            )
            .unwrap();
        assert!(
            input
                .accept(
                    1,
                    Event::Key {
                        code: 42,
                        pressed: true
                    }
                )
                .is_err()
        );
        assert!(input.keys.is_empty());
        input
            .accept(
                3,
                Event::Key {
                    code: 42,
                    pressed: true,
                },
            )
            .unwrap();
        assert!(input.keys.contains(&42));
    }
    #[test]
    fn revocation_releases_held_keys_and_blocks_late_input() {
        let mut input = InputGuard::new(Permissions {
            keyboard: true,
            ..Permissions::default()
        });
        input
            .accept(
                1,
                Event::Key {
                    code: 42,
                    pressed: true,
                },
            )
            .unwrap();
        let release = input.set_permissions(Permissions::default());
        assert!(release.contains(&Event::Key {
            code: 42,
            pressed: false
        }));
        assert!(
            input
                .accept(
                    2,
                    Event::Key {
                        code: 43,
                        pressed: true
                    }
                )
                .is_err()
        );
    }
}
