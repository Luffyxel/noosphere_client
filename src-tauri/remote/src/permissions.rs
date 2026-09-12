use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Principal {
    pub github_user_id: u64,
    pub identity_key: Vec<u8>,
    pub machine_id: [u8; 16],
}

impl Principal {
    pub fn validate(&self) -> Result<()> {
        if self.github_user_id == 0
            || self.identity_key.len() != 33
            || self.identity_key[0] != 5
            || self.machine_id == [0; 16]
        {
            return Err(Error::Authentication);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Permissions {
    pub screen: bool,
    pub keyboard: bool,
    pub mouse: bool,
    pub gamepad: bool,
    pub clipboard: bool,
    pub audio: bool,
}

impl Permissions {
    pub fn intersect(self, other: Self) -> Self {
        Self {
            screen: self.screen && other.screen,
            keyboard: self.keyboard && other.keyboard,
            mouse: self.mouse && other.mouse,
            gamepad: self.gamepad && other.gamepad,
            clipboard: self.clipboard && other.clipboard,
            audio: self.audio && other.audio,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Grant {
    pub principal: Principal,
    pub host_machine_id: [u8; 16],
    pub permissions: Permissions,
    pub unattended: bool,
}

#[derive(Default)]
pub struct PermissionStore {
    grants: BTreeMap<(Principal, [u8; 16]), Grant>,
    revision: u64,
}

impl PermissionStore {
    pub fn remember(&mut self, grant: Grant) -> Result<()> {
        grant.principal.validate()?;
        if grant.host_machine_id == [0; 16] {
            return Err(Error::PermissionDenied);
        }
        let key = (grant.principal.clone(), grant.host_machine_id);
        if self.grants.len() >= 100 && !self.grants.contains_key(&key) {
            return Err(Error::ResourceLimit);
        }
        self.grants.insert(key, grant);
        self.revision += 1;
        Ok(())
    }

    pub fn revoke(&mut self, principal: &Principal, host: [u8; 16]) {
        self.grants.remove(&(principal.clone(), host));
        self.revision += 1;
    }

    pub fn lookup(&self, principal: &Principal, host: [u8; 16]) -> Option<&Grant> {
        self.grants.get(&(principal.clone(), host))
    }

    pub fn unattended(
        &self,
        principal: &Principal,
        host: [u8; 16],
        requested: Permissions,
    ) -> Option<Permissions> {
        self.lookup(principal, host)
            .filter(|grant| grant.unattended)
            .map(|grant| grant.permissions.intersect(requested))
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn principal() -> Principal {
        let mut identity_key = vec![7; 33];
        identity_key[0] = 5;
        Principal {
            github_user_id: 42,
            identity_key,
            machine_id: [1; 16],
        }
    }

    #[test]
    fn grants_bind_both_machines_and_the_identity_not_a_username() {
        let mut store = PermissionStore::default();
        let peer = principal();
        let requested = Permissions {
            screen: true,
            keyboard: true,
            ..Permissions::default()
        };
        assert!(store.unattended(&peer, [2; 16], requested).is_none());
        store
            .remember(Grant {
                principal: peer.clone(),
                host_machine_id: [2; 16],
                permissions: Permissions {
                    screen: true,
                    ..Permissions::default()
                },
                unattended: true,
            })
            .unwrap();
        assert!(
            !store
                .unattended(&peer, [2; 16], requested)
                .unwrap()
                .keyboard
        );
        let mut changed = peer.clone();
        changed.identity_key[1] ^= 1;
        assert!(store.lookup(&changed, [2; 16]).is_none());
        changed = peer.clone();
        changed.machine_id = [3; 16];
        assert!(store.lookup(&changed, [2; 16]).is_none());
        assert!(store.lookup(&peer, [3; 16]).is_none());
        store.revoke(&peer, [2; 16]);
        assert!(store.unattended(&peer, [2; 16], requested).is_none());
    }

    #[test]
    fn remembering_does_not_implicitly_allow_unattended() {
        let mut store = PermissionStore::default();
        let peer = principal();
        store
            .remember(Grant {
                principal: peer.clone(),
                host_machine_id: [2; 16],
                permissions: Permissions::default(),
                unattended: false,
            })
            .unwrap();
        assert!(
            store
                .unattended(&peer, [2; 16], Permissions::default())
                .is_none()
        );
    }
}
