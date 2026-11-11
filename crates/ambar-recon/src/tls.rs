//! TLS inspection: protocol, cipher, certificate details and whether it would be trusted.

use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use serde::Serialize;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::TlsConnector;
use x509_parser::prelude::{FromDer, GeneralName, X509Certificate};

#[derive(Debug, Clone, Serialize)]
pub struct TlsInfo {
    pub target: String,
    pub protocol: String,
    pub cipher: String,
    pub subject: String,
    pub issuer: String,
    pub names: Vec<String>,
    pub not_after: String,
    pub days_left: i64,
    pub self_signed: bool,
    /// Would a browser using this computer's trusted CAs accept it for this name?
    pub trusted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trust_error: Option<String>,
    pub problems: Vec<String>,
}

/// Accepts every certificate so we can look at it; trust is judged separately.
#[derive(Debug)]
struct Capture {
    chain: Mutex<Vec<CertificateDer<'static>>>,
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl ServerCertVerifier for Capture {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if let Ok(mut c) = self.chain.lock() {
            *c = std::iter::once(end_entity)
                .chain(intermediates)
                .map(|c| c.clone().into_owned())
                .collect();
        }
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        d: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            m,
            c,
            d,
            &self.provider.signature_verification_algorithms,
        )
    }
    fn verify_tls13_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        d: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            m,
            c,
            d,
            &self.provider.signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// Inspect `host:port`. `connect_to` overrides the address (tests, or a resolved IP).
pub async fn inspect(
    host: &str,
    port: u16,
    connect_to: Option<SocketAddr>,
    roots: Option<RootCertStore>,
    wait: Duration,
) -> Result<TlsInfo> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let capture = Arc::new(Capture {
        chain: Mutex::new(Vec::new()),
        provider: provider.clone(),
    });
    let config = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(capture.clone())
        .with_no_client_auth();
    let name: ServerName<'static> = match host.parse::<IpAddr>() {
        Ok(ip) => ServerName::IpAddress(ip.into()),
        Err(_) => ServerName::try_from(host.to_string())
            .map_err(|_| anyhow!("invalid server name {host}"))?,
    };
    let addr = match connect_to {
        Some(a) => a,
        None => tokio::net::lookup_host((host, port))
            .await?
            .next()
            .ok_or_else(|| anyhow!("{host} doesn't resolve"))?,
    };
    let tcp = timeout(wait, TcpStream::connect(addr))
        .await
        .context("connection timed out")??;
    let tls = timeout(
        wait,
        TlsConnector::from(Arc::new(config)).connect(name.clone(), tcp),
    )
    .await
    .context("TLS handshake timed out")??;
    let (_, conn) = tls.get_ref();
    let protocol = conn
        .protocol_version()
        .map(|v| format!("{v:?}").replace('_', ".").replace("TLSv", "TLS "))
        .unwrap_or_default();
    let cipher = conn
        .negotiated_cipher_suite()
        .map(|c| format!("{:?}", c.suite()))
        .unwrap_or_default();
    let chain = capture.chain.lock().map(|c| c.clone()).unwrap_or_default();
    let leaf = chain
        .first()
        .ok_or_else(|| anyhow!("the server sent no certificate"))?;

    // Trust: the same check a client using this computer's CA store would make.
    let roots = match roots {
        Some(r) => r,
        None => {
            let mut r = RootCertStore::empty();
            for c in rustls_native_certs::load_native_certs().certs {
                let _ = r.add(c);
            }
            r
        }
    };
    let trust_error =
        match WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider).build() {
            Ok(v) => v
                .verify_server_cert(leaf, &chain[1..], &name, &[], UnixTime::now())
                .err()
                .map(|e| e.to_string()),
            Err(e) => Some(e.to_string()),
        };

    let (_, cert) = X509Certificate::from_der(leaf.as_ref())
        .map_err(|e| anyhow!("can't parse certificate: {e}"))?;
    let subject = cert.subject().to_string();
    let issuer = cert.issuer().to_string();
    let mut names: Vec<String> = Vec::new();
    if let Ok(Some(san)) = cert.subject_alternative_name() {
        for n in &san.value.general_names {
            match n {
                GeneralName::DNSName(d) => names.push(d.to_string()),
                GeneralName::IPAddress(b) if b.len() == 4 => {
                    names.push(IpAddr::from([b[0], b[1], b[2], b[3]]).to_string())
                }
                _ => {}
            }
        }
    }
    let not_after = cert.validity().not_after;
    let days_left = (not_after.timestamp() - chrono::Utc::now().timestamp()) / 86_400;
    let self_signed = subject == issuer;
    let mut problems = Vec::new();
    if days_left < 0 {
        problems.push(format!("certificate expired {} days ago", -days_left));
    } else if days_left < 30 {
        problems.push(format!("certificate expires in {days_left} days"));
    }
    if self_signed {
        problems.push("self-signed certificate".into());
    }
    if let Some(e) = &trust_error {
        problems.push(format!("not trusted: {e}"));
    }
    if protocol.contains("1.2") && !cipher.contains("ECDHE") {
        problems.push(format!("TLS 1.2 without forward secrecy ({cipher})"));
    }
    Ok(TlsInfo {
        target: format!("{host}:{port}"),
        protocol,
        cipher,
        subject,
        issuer,
        names,
        not_after: not_after.to_rfc2822().unwrap_or_default(),
        days_left,
        self_signed,
        trusted: trust_error.is_none(),
        trust_error,
        problems,
    })
}
