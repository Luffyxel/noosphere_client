pub mod assembler;
pub mod loopback;
pub mod packet;
pub mod session;

use crate::{
    Error, Result,
    signaling::{ALPN, MAX_CONTROL},
};
use quinn::{
    ClientConfig, ServerConfig, TransportConfig, VarInt,
    crypto::rustls::{QuicClientConfig, QuicServerConfig},
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::{sync::Arc, time::Duration};

#[derive(Clone, Copy)]
pub enum CongestionControl {
    Cubic,
    Bbr,
}

pub fn configuration(controller: CongestionControl) -> Arc<TransportConfig> {
    let mut transport = TransportConfig::default();
    transport.max_concurrent_uni_streams(VarInt::from_u32(0));
    transport.max_concurrent_bidi_streams(VarInt::from_u32(2));
    transport.stream_receive_window(VarInt::from_u32(MAX_CONTROL as u32));
    transport.receive_window(VarInt::from_u32(2 * MAX_CONTROL as u32));
    transport.datagram_receive_buffer_size(Some(256 * 1024));
    // Keep only a small send backlog: send_datagram evicts the oldest datagram when full.
    transport.datagram_send_buffer_size(16 * packet::MAX_DATAGRAM);
    transport.max_idle_timeout(Some(
        Duration::from_secs(10)
            .try_into()
            .expect("valid idle timeout"),
    ));
    transport.keep_alive_interval(Some(Duration::from_secs(2)));
    match controller {
        CongestionControl::Cubic => transport
            .congestion_controller_factory(Arc::new(quinn::congestion::CubicConfig::default())),
        CongestionControl::Bbr => transport
            .congestion_controller_factory(Arc::new(quinn::congestion::BbrConfig::default())),
    };
    Arc::new(transport)
}

pub fn server_config(
    certificate: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
    peer_certificate: CertificateDer<'static>,
    controller: CongestionControl,
) -> Result<ServerConfig> {
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(peer_certificate)
        .map_err(|_| Error::Authentication)?;
    let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
        Arc::new(roots),
        Arc::new(rustls::crypto::ring::default_provider()),
    )
    .build()
    .map_err(|_| Error::Authentication)?;
    let mut tls = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .map_err(|_| Error::Authentication)?
    .with_client_cert_verifier(verifier)
    .with_single_cert(vec![certificate], key)
    .map_err(|_| Error::Authentication)?;
    tls.alpn_protocols = vec![ALPN.to_vec()];
    tls.max_early_data_size = 0;
    let mut config = ServerConfig::with_crypto(Arc::new(
        QuicServerConfig::try_from(tls).map_err(|_| Error::Authentication)?,
    ));
    config.transport_config(configuration(controller));
    Ok(config)
}

pub fn client_config(
    certificate: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
    peer_certificate: CertificateDer<'static>,
    controller: CongestionControl,
) -> Result<ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(peer_certificate)
        .map_err(|_| Error::Authentication)?;
    let mut tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .map_err(|_| Error::Authentication)?
    .with_root_certificates(roots)
    .with_client_auth_cert(vec![certificate], key)
    .map_err(|_| Error::Authentication)?;
    tls.alpn_protocols = vec![ALPN.to_vec()];
    tls.enable_early_data = false;
    let mut config = ClientConfig::new(Arc::new(
        QuicClientConfig::try_from(tls).map_err(|_| Error::Authentication)?,
    ));
    config.transport_config(configuration(controller));
    Ok(config)
}

pub fn exporter(connection: &quinn::Connection) -> Result<[u8; 32]> {
    let mut key = [0; 32];
    connection
        .export_keying_material(&mut key, b"noosphere/remote/session/v1", ALPN)
        .map_err(|_| Error::Authentication)?;
    Ok(key)
}
