//! Host-owned access decisions. A GitHub account can own several distinct machines;
//! account equality is never an authorization shortcut.
use crate::{
    Error, Result,
    permissions::{Grant, Permissions, Principal},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccessRequest {
    pub id: [u8; 32],
    pub guest: Principal,
    pub host: Principal,
    pub permissions: Permissions,
    pub created_at: u64,
    pub expires_at: u64,
}

impl AccessRequest {
    pub fn validate(&self, now: u64) -> Result<()> {
        self.guest.validate()?;
        self.host.validate()?;
        if self.guest.machine_id == self.host.machine_id
            || self.id == [0; 32]
            || !self.permissions.screen
            || self.created_at > now.saturating_add(30)
            || self.expires_at <= now
            || self.expires_at <= self.created_at
            || self.expires_at - self.created_at > 120
        {
            return Err(Error::PermissionDenied);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Decision {
    Pending,
    Approved,
    Declined,
    Revoked,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccessDecision {
    pub request: AccessRequest,
    pub decision: Decision,
    pub permissions: Permissions,
    pub automatic: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccessBook {
    pub enabled: bool,
    pub revision: u64,
    pub grants: Vec<Grant>,
    pub requests: Vec<AccessDecision>,
}

impl AccessBook {
    pub fn grant(&self, guest: &Principal, host: [u8; 16]) -> Option<&Grant> {
        self.grants
            .iter()
            .find(|g| &g.principal == guest && g.host_machine_id == host)
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.revision = self.revision.saturating_add(1);
        if !enabled {
            for request in &mut self.requests {
                request.decision = Decision::Revoked;
                request.permissions = Permissions::default();
            }
        }
    }

    pub fn set_grant(&mut self, grant: Grant, owner: &Principal) -> Result<()> {
        grant.principal.validate()?;
        if grant.host_machine_id != owner.machine_id
            || grant.principal.machine_id == owner.machine_id
            || (!grant.permissions.screen && grant.unattended)
        {
            return Err(Error::PermissionDenied);
        }
        let existing = self.grants.iter().position(|g| {
            g.principal == grant.principal && g.host_machine_id == grant.host_machine_id
        });
        if existing.is_none() && self.grants.len() >= 100 {
            return Err(Error::ResourceLimit);
        }
        // Changes can reduce an existing authorization immediately, never silently escalate it.
        for request in &mut self.requests {
            if request.request.guest == grant.principal {
                request.permissions = request.permissions.intersect(grant.permissions);
                if !request.permissions.screen && request.decision == Decision::Approved {
                    request.decision = Decision::Revoked;
                }
            }
        }
        if let Some(index) = existing {
            self.grants[index] = grant;
        } else {
            self.grants.push(grant);
        }
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    pub fn revoke(&mut self, guest: &Principal) {
        self.grants.retain(|g| &g.principal != guest);
        self.cancel(guest);
        self.revision = self.revision.saturating_add(1);
    }

    pub fn cancel(&mut self, guest: &Principal) {
        for request in &mut self.requests {
            if &request.request.guest == guest
                && matches!(request.decision, Decision::Pending | Decision::Approved)
            {
                request.decision = Decision::Revoked;
                request.permissions = Permissions::default();
                self.revision = self.revision.saturating_add(1);
            }
        }
    }

    pub fn receive(
        &mut self,
        request: AccessRequest,
        owner: &Principal,
        now: u64,
    ) -> Result<AccessDecision> {
        request.validate(now)?;
        if &request.host != owner || !self.enabled {
            return Err(Error::PermissionDenied);
        }
        self.requests.retain(|r| r.request.expires_at > now);
        if let Some(previous) = self.requests.iter().find(|r| r.request.id == request.id) {
            if previous.request != request {
                return Err(Error::Authentication);
            }
            return Ok(previous.clone());
        }
        if self.requests.len() >= 100 {
            return Err(Error::ResourceLimit);
        }
        self.cancel(&request.guest);
        let grant = self.grant(&request.guest, owner.machine_id);
        let automatic = grant.is_some_and(|g| g.unattended && g.permissions.screen);
        let permissions = if automatic {
            grant.unwrap().permissions.intersect(request.permissions)
        } else {
            Permissions::default()
        };
        let decision = AccessDecision {
            request,
            decision: if automatic {
                Decision::Approved
            } else {
                Decision::Pending
            },
            permissions,
            automatic,
        };
        self.requests.push(decision.clone());
        self.revision = self.revision.saturating_add(1);
        Ok(decision)
    }

    pub fn decide(
        &mut self,
        id: [u8; 32],
        permissions: Permissions,
        remember: bool,
        unattended: bool,
        owner: &Principal,
        now: u64,
    ) -> Result<()> {
        if !self.enabled || (unattended && (!remember || !permissions.screen)) {
            return Err(Error::PermissionDenied);
        }
        let index = self
            .requests
            .iter()
            .position(|r| r.request.id == id)
            .ok_or(Error::PermissionDenied)?;
        let request = self.requests[index].request.clone();
        request.validate(now)?;
        if request.host != *owner || self.requests[index].decision != Decision::Pending {
            return Err(Error::PermissionDenied);
        }
        let permissions = permissions.intersect(request.permissions);
        if remember && permissions.screen {
            self.set_grant(
                Grant {
                    principal: request.guest.clone(),
                    host_machine_id: owner.machine_id,
                    permissions,
                    unattended,
                },
                owner,
            )?;
        }
        self.requests[index].decision = if permissions.screen {
            Decision::Approved
        } else {
            Decision::Declined
        };
        self.requests[index].permissions = permissions;
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn principal(machine: u8) -> Principal {
        Principal {
            github_user_id: 42,
            identity_key: [vec![5], vec![machine; 32]].concat(),
            machine_id: [machine; 16],
        }
    }
    fn request() -> AccessRequest {
        AccessRequest {
            id: [8; 32],
            guest: principal(2),
            host: principal(1),
            permissions: Permissions {
                screen: true,
                keyboard: true,
                mouse: true,
                ..Default::default()
            },
            created_at: 100,
            expires_at: 220,
        }
    }
    #[test]
    fn same_account_requires_distinct_machines_and_explicit_authorization() {
        let mut book = AccessBook::default();
        assert!(book.receive(request(), &principal(1), 101).is_err());
        book.set_enabled(true);
        assert_eq!(
            book.receive(request(), &principal(1), 101)
                .unwrap()
                .decision,
            Decision::Pending
        );
        let mut same = request();
        same.guest.machine_id = same.host.machine_id;
        assert!(book.receive(same, &principal(1), 101).is_err());
    }

    #[test]
    fn cancellation_preserves_saved_grants_and_cannot_be_replayed() {
        let mut book = AccessBook::default();
        book.set_enabled(true);
        book.receive(request(), &principal(1), 101).unwrap();
        book.decide(
            [8; 32],
            request().permissions,
            true,
            true,
            &principal(1),
            101,
        )
        .unwrap();
        book.cancel(&principal(2));
        assert!(book.grant(&principal(2), [1; 16]).unwrap().unattended);
        assert_eq!(
            book.receive(request(), &principal(1), 102)
                .unwrap()
                .decision,
            Decision::Revoked
        );
        let revision = book.revision;
        book.cancel(&principal(2));
        assert_eq!(book.revision, revision);
        let mut next = request();
        next.id = [9; 32];
        assert_eq!(
            book.receive(next, &principal(1), 102).unwrap().decision,
            Decision::Approved
        );
        book.set_enabled(false);
        assert!(
            book.requests
                .iter()
                .all(|r| r.decision == Decision::Revoked)
        );
    }
    #[test]
    fn unattended_is_exactly_scoped_and_revocation_is_immediate() {
        let mut book = AccessBook::default();
        book.set_enabled(true);
        book.set_grant(
            Grant {
                principal: principal(2),
                host_machine_id: [1; 16],
                permissions: Permissions {
                    screen: true,
                    mouse: true,
                    ..Default::default()
                },
                unattended: true,
            },
            &principal(1),
        )
        .unwrap();
        let decision = book.receive(request(), &principal(1), 101).unwrap();
        assert_eq!(decision.decision, Decision::Approved);
        assert!(!decision.permissions.keyboard);
        assert!(decision.permissions.mouse);
        let mut changed = request();
        changed.id = [9; 32];
        changed.guest.identity_key[5] ^= 1;
        assert_eq!(
            book.receive(changed, &principal(1), 101).unwrap().decision,
            Decision::Pending
        );
        book.revoke(&principal(2));
        assert_eq!(
            book.receive(request(), &principal(1), 101)
                .unwrap()
                .decision,
            Decision::Revoked
        );
    }
    #[test]
    fn remembered_permissions_do_not_imply_unattended_and_replay_cannot_reopen_a_denial() {
        let mut book = AccessBook::default();
        book.set_enabled(true);
        book.receive(request(), &principal(1), 101).unwrap();
        book.decide(
            [8; 32],
            Permissions::default(),
            false,
            false,
            &principal(1),
            101,
        )
        .unwrap();
        assert_eq!(
            book.receive(request(), &principal(1), 102)
                .unwrap()
                .decision,
            Decision::Declined
        );
        assert!(
            book.decide(
                [8; 32],
                request().permissions,
                true,
                true,
                &principal(1),
                102
            )
            .is_err()
        );
        let mut next = request();
        next.id = [9; 32];
        book.receive(next.clone(), &principal(1), 102).unwrap();
        book.decide([9; 32], next.permissions, true, false, &principal(1), 102)
            .unwrap();
        next.id = [10; 32];
        assert_eq!(
            book.receive(next, &principal(1), 103).unwrap().decision,
            Decision::Pending
        );
        let restored: AccessBook =
            serde_json::from_slice(&serde_json::to_vec(&book).unwrap()).unwrap();
        assert!(!restored.grants[0].unattended);
        assert!(request().validate(220).is_err());
    }
}
