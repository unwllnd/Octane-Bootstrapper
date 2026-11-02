use std::sync::Arc;

use anyhow::Result;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::crypto::ring;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, Error, RootCertStore, SignatureScheme};
use rustls_platform_verifier::Verifier;

#[derive(Debug)]
struct Roots {
    bundled: Arc<WebPkiServerVerifier>,
    system: Verifier,
}


pub fn config() -> Result<ClientConfig> {
    let provider = Arc::new(ring::default_provider());
    let mut known = RootCertStore::empty();
    known.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let bundled = WebPkiServerVerifier::builder_with_provider(Arc::new(known), provider.clone());
    let roots = Roots { bundled: bundled.build()?, system: Verifier::new(provider.clone())? };

    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(roots))
        .with_no_client_auth();
    Ok(config)
}


impl ServerCertVerifier for Roots {
    fn verify_server_cert(
        &self,
        certificate: &CertificateDer,
        intermediates: &[CertificateDer],
        name: &ServerName,
        ocsp: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let bundled = self.bundled.verify_server_cert(certificate, intermediates, name, ocsp, now);
        if bundled.is_ok() {
            return bundled;
        }
        self.system.verify_server_cert(certificate, intermediates, name, ocsp, now)
    }


    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.bundled.verify_tls12_signature(message, certificate, signature)
    }


    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.bundled.verify_tls13_signature(message, certificate, signature)
    }


    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.bundled.supported_verify_schemes()
    }
}
