//! Real authenticated UDP endpoints for the on-device diagnostic. No external listener.
use super::{self as transport, CongestionControl, session::AuthenticatedTransport};
use crate::{
    Error, Result,
    identity::SignalIdentity,
    permissions::Permissions,
    signaling::{SessionBinding, random_id},
};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Loopback {
    pub host: AuthenticatedTransport,
    pub guest: AuthenticatedTransport,
    _server: quinn::Endpoint,
    _client: quinn::Endpoint,
}
impl Loopback {
    pub async fn open(identity: &SignalIdentity, machine_id: [u8; 16]) -> Result<Self> {
        let certificate = || {
            rcgen::generate_simple_self_signed(vec!["localhost".into()])
                .map_err(|_| Error::Authentication)
        };
        let host = certificate()?;
        let guest = certificate()?;
        let host_der: CertificateDer<'static> = host.cert.into();
        let guest_der: CertificateDer<'static> = guest.cert.into();
        let server_config = transport::server_config(
            host_der.clone(),
            PrivatePkcs8KeyDer::from(host.signing_key.serialize_der()).into(),
            guest_der.clone(),
            CongestionControl::Cubic,
        )?;
        let client_config = transport::client_config(
            guest_der.clone(),
            PrivatePkcs8KeyDer::from(guest.signing_key.serialize_der()).into(),
            host_der.clone(),
            CongestionControl::Cubic,
        )?;
        let server = quinn::Endpoint::server(server_config, ([127, 0, 0, 1], 0).into())?;
        let mut client = quinn::Endpoint::client(([127, 0, 0, 1], 0).into())?;
        client.set_default_client_config(client_config);
        let connecting = client
            .connect(server.local_addr()?, "localhost")
            .map_err(|_| Error::Authentication)?;
        let (a, b) = tokio::join!(
            async {
                server
                    .accept()
                    .await
                    .ok_or(Error::Authentication)?
                    .await
                    .map_err(|_| Error::Authentication)
            },
            async { connecting.await.map_err(|_| Error::Authentication) }
        );
        let digest = |cert: &CertificateDer<'_>| {
            ring::digest::digest(&ring::digest::SHA256, cert)
                .as_ref()
                .try_into()
                .map_err(|_| Error::Authentication)
        };
        let binding = SessionBinding {
            session_id: random_id()?,
            host: identity.principal(machine_id)?,
            guest: identity.principal(machine_id)?,
            host_certificate_sha256: digest(&host_der)?,
            guest_certificate_sha256: digest(&guest_der)?,
            expires_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| Error::Authentication)?
                .as_secs()
                + 60,
        };
        let permissions = Permissions {
            screen: true,
            keyboard: true,
            mouse: true,
            ..Permissions::default()
        };
        let (a, b) = tokio::join!(
            AuthenticatedTransport::establish(a?, &binding, true, identity, permissions),
            AuthenticatedTransport::establish(b?, &binding, false, identity, permissions)
        );
        Ok(Self {
            host: a?,
            guest: b?,
            _server: server,
            _client: client,
        })
    }
}
impl Drop for Loopback {
    fn drop(&mut self) {
        self.host.close();
        self.guest.close();
    }
}
