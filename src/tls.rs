use std::sync::Arc;

use anyhow::Result;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::crypto::ring;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, Error, RootCertStore, SignatureScheme};
use rustls_platform_verifier::Verifier;

#[derive(Debug)]
struct TrustedRoots {
    system: Verifier,
    bundled: Arc<WebPkiServerVerifier>,
}

impl ServerCertVerifier for TrustedRoots {
    fn verify_server_cert(&self, end_entity: &CertificateDer<'_>, intermediates: &[CertificateDer<'_>], server_name: &ServerName<'_>, ocsp: &[u8], now: UnixTime) -> Result<ServerCertVerified, Error> {
        self.bundled.verify_server_cert(end_entity, intermediates, server_name, ocsp, now).or_else(|_| self.system.verify_server_cert(end_entity, intermediates, server_name, ocsp, now))
    }

    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, signature: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        self.bundled.verify_tls12_signature(message, cert, signature)
    }

    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, signature: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        self.bundled.verify_tls13_signature(message, cert, signature)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.bundled.supported_verify_schemes()
    }
}

pub fn config() -> Result<ClientConfig> {
    let provider = Arc::new(ring::default_provider());
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let bundled = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider.clone()).build()?;
    let verifier = TrustedRoots { system: Verifier::new(provider.clone())?, bundled };
    Ok(ClientConfig::builder_with_provider(provider).with_safe_default_protocol_versions()?.dangerous().with_custom_certificate_verifier(Arc::new(verifier)).with_no_client_auth())
}
