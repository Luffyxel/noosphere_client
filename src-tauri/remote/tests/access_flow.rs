use noosphere_remote::{
    access::{AccessBook, AccessRequest, Decision},
    directory::{Registration, Sealed},
    identity::SignalIdentity,
    input::{Event, InputGuard},
    permissions::Permissions,
};

#[test]
fn two_machines_on_one_account_exchange_an_encrypted_request_and_enforce_saved_rights() {
    let host = SignalIdentity::from_private_key(42, &[1; 32]).unwrap();
    let client = SignalIdentity::from_private_key(42, &[2; 32]).unwrap();
    let host_principal = host.principal([1; 16]).unwrap();
    let client_principal = client.principal([2; 16]).unwrap();
    let registration = Registration::create(&client, [2; 16], 420).unwrap();
    registration.verify(42, 420).unwrap();
    let requested = Permissions {
        screen: true,
        keyboard: true,
        mouse: true,
        ..Default::default()
    };
    let request = AccessRequest {
        id: [7; 32],
        guest: client_principal.clone(),
        host: host_principal.clone(),
        permissions: requested,
        created_at: 100,
        expires_at: 220,
    };
    let envelope = Sealed::seal(
        &client,
        [2; 16],
        host_principal.clone(),
        &serde_json::to_vec(&request).unwrap(),
    )
    .unwrap();
    let clear = envelope.open(&host, [1; 16], &client_principal).unwrap();
    let received: AccessRequest = serde_json::from_slice(&clear).unwrap();
    let mut book = AccessBook::default();
    book.set_enabled(true);
    assert_eq!(
        book.receive(received, &host_principal, 101)
            .unwrap()
            .decision,
        Decision::Pending
    );
    let allowed = Permissions {
        screen: true,
        mouse: true,
        ..Default::default()
    };
    book.decide(request.id, allowed, true, true, &host_principal, 101)
        .unwrap();
    let persisted = serde_json::to_vec(&book).unwrap();
    let mut restored: AccessBook = serde_json::from_slice(&persisted).unwrap();
    let next = AccessRequest {
        id: [8; 32],
        ..request
    };
    let decision = restored.receive(next, &host_principal, 102).unwrap();
    assert_eq!(decision.decision, Decision::Approved);
    assert!(decision.automatic);
    let answer = Sealed::seal(
        &host,
        [1; 16],
        client_principal.clone(),
        &serde_json::to_vec(&decision).unwrap(),
    )
    .unwrap();
    answer.open(&client, [2; 16], &host_principal).unwrap();
    let mut input = InputGuard::new(decision.permissions);
    assert!(
        input
            .accept(
                1,
                Event::Key {
                    code: 65,
                    pressed: true
                }
            )
            .is_err()
    );
    input
        .accept(
            2,
            Event::Button {
                button: 0,
                pressed: true,
            },
        )
        .unwrap();
    restored.revoke(&client_principal);
    let release = input.set_permissions(restored.requests.last().unwrap().permissions);
    assert!(release.contains(&Event::Button {
        button: 0,
        pressed: false
    }));
    assert!(input.accept(3, Event::Pointer { x: 12, y: 24 }).is_err());
}

#[test]
fn another_machine_or_changed_identity_never_inherits_same_account_access() {
    let host = SignalIdentity::from_private_key(42, &[1; 32]).unwrap();
    let client = SignalIdentity::from_private_key(42, &[2; 32]).unwrap();
    let owner = host.principal([1; 16]).unwrap();
    let mut book = AccessBook::default();
    book.set_enabled(true);
    let permissions = Permissions {
        screen: true,
        ..Default::default()
    };
    book.set_grant(
        noosphere_remote::permissions::Grant {
            principal: client.principal([2; 16]).unwrap(),
            host_machine_id: owner.machine_id,
            permissions,
            unattended: true,
        },
        &owner,
    )
    .unwrap();
    for (index, guest) in [
        client.principal([3; 16]).unwrap(),
        SignalIdentity::from_private_key(42, &[3; 32])
            .unwrap()
            .principal([2; 16])
            .unwrap(),
    ]
    .into_iter()
    .enumerate()
    {
        let decision = book
            .receive(
                AccessRequest {
                    id: [index as u8 + 1; 32],
                    guest,
                    host: owner.clone(),
                    permissions,
                    created_at: 100,
                    expires_at: 220,
                },
                &owner,
                101,
            )
            .unwrap();
        assert_eq!(decision.decision, Decision::Pending);
        assert!(!decision.automatic);
    }
}
