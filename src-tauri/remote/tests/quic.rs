use bytes::Bytes;
use noosphere_remote::identity::SignalIdentity;
use noosphere_remote::{
    permissions::{Permissions, Principal},
    signaling::SessionBinding,
    transport::{
        self, CongestionControl,
        packet::packetize,
        session::{AuthenticatedTransport, IdentityAuthenticator},
    },
};
use rand::{TryRngCore as _, rngs::OsRng};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct Identity(SignalIdentity);
impl Identity {
    fn new(user: u64) -> Self {
        let pair = libsignal_protocol::KeyPair::generate(&mut OsRng.unwrap_err());
        Self(SignalIdentity::from_private_key(user, &pair.private_key.serialize()).unwrap())
    }
    fn principal(&self, user: u64) -> Principal {
        self.0.principal([user as u8; 16]).unwrap()
    }
}
impl IdentityAuthenticator for Identity {
    fn sign(&self, bytes: &[u8]) -> noosphere_remote::Result<Vec<u8>> {
        self.0.sign(bytes)
    }
    fn verify(
        &self,
        principal: &Principal,
        bytes: &[u8],
        proof: &[u8],
    ) -> noosphere_remote::Result<()> {
        self.0.verify(principal, bytes, proof)
    }
}

async fn endpoints() -> (
    quinn::Endpoint,
    quinn::Endpoint,
    quinn::Connection,
    quinn::Connection,
    SessionBinding,
    Identity,
    Identity,
) {
    let host = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let guest = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let host_der: CertificateDer<'static> = host.cert.into();
    let guest_der: CertificateDer<'static> = guest.cert.into();
    let host_config = transport::server_config(
        host_der.clone(),
        PrivatePkcs8KeyDer::from(host.signing_key.serialize_der()).into(),
        guest_der.clone(),
        CongestionControl::Cubic,
    )
    .unwrap();
    let guest_config = transport::client_config(
        guest_der.clone(),
        PrivatePkcs8KeyDer::from(guest.signing_key.serialize_der()).into(),
        host_der.clone(),
        CongestionControl::Cubic,
    )
    .unwrap();
    let server = quinn::Endpoint::server(host_config, "127.0.0.1:0".parse().unwrap()).unwrap();
    let mut client = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    client.set_default_client_config(guest_config);
    let connecting = client
        .connect(server.local_addr().unwrap(), "localhost")
        .unwrap();
    let (a, b) = tokio::join!(
        async { server.accept().await.unwrap().await.unwrap() },
        connecting
    );
    let b = b.unwrap();
    assert_eq!(
        transport::exporter(&a).unwrap(),
        transport::exporter(&b).unwrap()
    );
    let host_identity = Identity::new(42);
    let guest_identity = Identity::new(99);
    let digest = |cert: &CertificateDer<'_>| {
        ring::digest::digest(&ring::digest::SHA256, cert)
            .as_ref()
            .try_into()
            .unwrap()
    };
    let binding = SessionBinding {
        session_id: [4; 32],
        host: host_identity.principal(42),
        guest: guest_identity.principal(99),
        host_certificate_sha256: digest(&host_der),
        guest_certificate_sha256: digest(&guest_der),
        expires_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 60,
    };
    (server, client, a, b, binding, host_identity, guest_identity)
}

#[tokio::test]
async fn two_endpoints_authenticate_and_exchange_media_datagrams() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let (_server, _client, a, b, binding, host, guest) = endpoints().await;
        let permissions = Permissions {
            screen: true,
            ..Permissions::default()
        };
        let (a, b) = tokio::join!(
            AuthenticatedTransport::establish(a, &binding, true, &host, permissions),
            AuthenticatedTransport::establish(b, &binding, false, &guest, permissions)
        );
        let a = a.unwrap();
        let mut b = b.unwrap();
        let packet = packetize(
            Bytes::from_static(b"encoded-frame"),
            0,
            0,
            0,
            30_000,
            true,
            1200,
        )
        .unwrap()
        .remove(0);
        assert!(a.send(packet.clone(), 100).unwrap());
        assert_eq!(b.receive(|| 200).await.unwrap().unwrap(), packet);
        assert!(
            !a.send_before_deadline(packet.clone(), || 30_001)
                .await
                .unwrap()
        );
        let large = Bytes::from(vec![0x5a; 64 * 1024]);
        let packets = packetize(
            large.clone(),
            1,
            1,
            0,
            250_000,
            true,
            a.max_datagram_size().unwrap(),
        )
        .unwrap();
        let mut assembler = transport::assembler::Assembler::default();
        let (_, received) = tokio::join!(
            async {
                for packet in packets {
                    assert!(a.send_before_deadline(packet, || 1).await.unwrap());
                }
            },
            async {
                loop {
                    if let Some(packet) = b.receive(|| 2).await.unwrap()
                        && let Some(frame) = assembler.receive(packet, 2).unwrap()
                    {
                        break frame;
                    }
                }
            }
        );
        assert_eq!(received.data, large);
        b.set_permissions(Permissions::default());
        a.send(packet, 100).unwrap();
        assert!(b.receive(|| 200).await.is_err());
        a.close();
        b.close();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn valid_tls_certificate_cannot_substitute_another_noosphere_identity() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let (_server, _client, a, b, binding, host, _guest) = endpoints().await;
        let attacker = Identity::new(99);
        let (a, _b) = tokio::join!(
            AuthenticatedTransport::establish(a, &binding, true, &host, Permissions::default()),
            AuthenticatedTransport::establish(
                b,
                &binding,
                false,
                &attacker,
                Permissions::default()
            )
        );
        assert!(a.is_err());
    })
    .await
    .unwrap();
}
