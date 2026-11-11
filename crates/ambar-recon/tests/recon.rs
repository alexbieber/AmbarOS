//! The real `ambar-recon` binary against real local servers.

use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn recon(args: &[&str], envs: &[(&str, &str)]) -> (i32, Vec<Value>, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_ambar-recon"));
    c.args(args).env_remove("AMBAR_SCOPE");
    for (k, v) in envs {
        c.env(k, v);
    }
    let o = c
        .output()
        .unwrap_or_else(|e| panic!("run ambar-recon: {e}"));
    let stdout = String::from_utf8_lossy(&o.stdout).to_string();
    let json = stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    (
        o.status.code().unwrap_or(-1),
        json,
        stdout + &String::from_utf8_lossy(&o.stderr),
    )
}

/// A server that speaks first, like SSH.
async fn banner_server(banner: &'static [u8]) -> Result<u16> {
    let l = TcpListener::bind("127.0.0.1:0").await?;
    let port = l.local_addr()?.port();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = l.accept().await {
            let _ = s.write_all(banner).await;
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    });
    Ok(port)
}

/// A server that waits for the client, like HTTP.
async fn quiet_http_server() -> Result<u16> {
    let l = TcpListener::bind("127.0.0.1:0").await?;
    let port = l.local_addr()?.port();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = l.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 256];
                if s.read(&mut buf).await.unwrap_or(0) > 0 {
                    let _ = s.write_all(b"HTTP/1.1 200 OK\r\nServer: AmbarTestd/1.0\r\nContent-Length: 0\r\n\r\n").await;
                }
            });
        }
    });
    Ok(port)
}

#[tokio::test(flavor = "multi_thread")]
async fn ports_finds_open_ports_and_their_services() -> Result<()> {
    let ssh = banner_server(b"SSH-2.0-OpenSSH_9.6 AmbarTest\r\n").await?;
    let web = quiet_http_server().await?;
    let closed = TcpListener::bind("127.0.0.1:0").await?.local_addr()?.port(); // bound then dropped: closed
    let spec = format!("{ssh},{web},{closed}");
    let (code, out, raw) = tokio::task::spawn_blocking(move || {
        recon(
            &[
                "--json",
                "ports",
                "127.0.0.1",
                "-p",
                &spec,
                "--timeout-ms",
                "800",
            ],
            &[],
        )
    })
    .await?;
    assert_eq!(code, 0, "{raw}");
    let ports: Vec<u64> = out.iter().filter_map(|v| v["port"].as_u64()).collect();
    assert_eq!(
        ports,
        vec![ssh.min(web) as u64, ssh.max(web) as u64],
        "{raw}"
    );
    let by_port = |p: u16| {
        out.iter()
            .find(|v| v["port"] == p)
            .cloned()
            .unwrap_or_default()
    };
    assert_eq!(by_port(ssh)["service"], "ssh");
    assert!(
        by_port(ssh)["banner"]
            .as_str()
            .unwrap_or("")
            .contains("OpenSSH_9.6")
    );
    assert_eq!(by_port(web)["service"], "http");
    assert!(
        by_port(web)["banner"]
            .as_str()
            .unwrap_or("")
            .contains("Server: AmbarTestd/1.0"),
        "{raw}"
    );
    Ok(())
}

#[test]
fn other_computers_need_a_scope_file() {
    let (code, out, raw) = recon(&["--json", "ports", "192.0.2.10", "-p", "80"], &[]);
    assert_eq!(code, 0, "{raw}");
    assert_eq!(out.len(), 1, "{raw}");
    assert_eq!(out[0]["refused"], "192.0.2.10");
    assert!(
        out[0]["reason"]
            .as_str()
            .unwrap_or("")
            .contains("scope file")
    );
}

#[test]
fn scope_file_rules_and_dates() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let scope = dir.path().join("scope.toml");
    std::fs::write(
        &scope,
        r#"
        [engagement]
        name = "test"
        starts = "2026-11-01"
        ends = "2026-11-30"
        [scope]
        include = ["192.0.2.0/24", "*.acme.example"]
        exclude = ["192.0.2.1"]
    "#,
    )?;
    let s = scope.to_str().unwrap_or_default();
    let (_, out, raw) = recon(
        &[
            "--json",
            "--scope",
            s,
            "scope",
            "192.0.2.7",
            "192.0.2.1",
            "198.51.100.1",
            "www.acme.example",
        ],
        &[("AMBAR_RECON_TODAY", "2026-11-12")],
    );
    let ok: Vec<_> = out.iter().filter_map(|v| v["in_scope"].as_str()).collect();
    let no: Vec<_> = out.iter().filter_map(|v| v["refused"].as_str()).collect();
    assert_eq!(ok, ["192.0.2.7", "www.acme.example"], "{raw}");
    assert_eq!(no, ["192.0.2.1", "198.51.100.1"], "{raw}");
    let (_, out, _) = recon(
        &["--json", "--scope", s, "scope", "192.0.2.7"],
        &[("AMBAR_RECON_TODAY", "2026-12-02")],
    );
    assert!(out[0]["reason"].as_str().unwrap_or("").contains("ended"));
    // Scope checks touch nothing and aren't logged; scans are logged next to the scope file.
    assert!(!dir.path().join("activity.log").exists());
    let _ = recon(
        &["--json", "--scope", s, "ports", "127.0.0.1", "-p", "9"],
        &[("AMBAR_RECON_TODAY", "2026-11-12")],
    );
    let log = std::fs::read_to_string(dir.path().join("activity.log"))?;
    assert_eq!(log.lines().count(), 1);
    assert!(log.contains("ambar-recon --json --scope") && log.contains("ports 127.0.0.1"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn http_fingerprint() -> Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("server", "nginx/1.27.0")
                .insert_header("x-powered-by", "PHP/8.3")
                .insert_header("x-frame-options", "DENY")
                .insert_header("set-cookie", "session=abc; Path=/")
                .set_body_string("<html><head><title>ACME Shop</title></head><link href='/wp-content/themes/x.css'></html>"),
        )
        .mount(&server)
        .await;
    let url = server.uri();
    let (code, out, raw) =
        tokio::task::spawn_blocking(move || recon(&["--json", "http", &url], &[])).await?;
    assert_eq!(code, 0, "{raw}");
    let v = &out[0];
    assert_eq!(v["status"], 200);
    assert_eq!(v["title"], "ACME Shop");
    let tech: Vec<_> = v["technologies"]
        .as_array()
        .map(|a| a.iter().filter_map(|t| t.as_str()).collect())
        .unwrap_or_default();
    assert_eq!(tech, ["nginx", "PHP", "WordPress"], "{raw}");
    let missing: Vec<_> = v["missing_security_headers"]
        .as_array()
        .map(|a| a.iter().filter_map(|t| t.as_str()).collect())
        .unwrap_or_default();
    assert!(
        missing.contains(&"content-security-policy") && !missing.contains(&"x-frame-options"),
        "{raw}"
    );
    assert!(
        !missing.contains(&"strict-transport-security"),
        "HSTS only matters over HTTPS"
    );
    assert_eq!(v["weak_cookies"][0], "session");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn subdomains_from_certificate_transparency() -> Result<()> {
    let ct = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {"name_value": "www.acme.example\n*.acme.example"},
            {"name_value": "vpn.acme.example"},
            {"name_value": "acme.example.attacker.test"}
        ])))
        .mount(&ct)
        .await;
    let dir = tempfile::tempdir()?;
    let scope = dir.path().join("scope.toml");
    std::fs::write(
        &scope,
        "[scope]\ninclude = [\"*.acme.example\"]\nexclude = [\"vpn.acme.example\"]\n",
    )?;
    let (s, base) = (scope.to_str().unwrap_or_default().to_string(), ct.uri());
    let (code, out, raw) = tokio::task::spawn_blocking(move || {
        recon(
            &[
                "--json",
                "--scope",
                &s,
                "subdomains",
                "acme.example",
                "--no-guess",
            ],
            &[("AMBAR_RECON_CT_URL", &base)],
        )
    })
    .await?;
    assert_eq!(code, 0, "{raw}");
    let names: Vec<_> = out.iter().filter_map(|v| v["name"].as_str()).collect();
    assert_eq!(
        names,
        ["acme.example", "vpn.acme.example", "www.acme.example"],
        "{raw}"
    );
    let vpn = out
        .iter()
        .find(|v| v["name"] == "vpn.acme.example")
        .cloned()
        .unwrap_or_default();
    assert_eq!(vpn["in_scope"], false);
    Ok(())
}

#[test]
fn subdomains_of_an_out_of_scope_domain_are_refused() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let scope = dir.path().join("scope.toml");
    std::fs::write(&scope, "[scope]\ninclude = [\"acme.example\"]\n")?;
    let (_, out, raw) = recon(
        &[
            "--json",
            "--scope",
            scope.to_str().unwrap_or_default(),
            "subdomains",
            "other.example",
            "--no-ct",
        ],
        &[],
    );
    assert!(out[0]["refused"] == "other.example", "{raw}");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn tls_inspection_with_a_real_certificate_chain() -> Result<()> {
    use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, KeyPair};
    // A private CA and a leaf for "localhost" that expires in 10 days.
    let ca_key = KeyPair::generate()?;
    let mut ca_params = CertificateParams::new(Vec::<String>::new())?;
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "Ambar Test CA");
    let ca = ca_params.self_signed(&ca_key)?;
    let leaf_key = KeyPair::generate()?;
    let mut leaf_params = CertificateParams::new(vec!["localhost".to_string()])?;
    leaf_params
        .distinguished_name
        .push(DnType::CommonName, "localhost");
    leaf_params.not_after = time_in_days(10);
    let issuer = rcgen::Issuer::new(ca_params, ca_key);
    let leaf = leaf_params.signed_by(&leaf_key, &issuer)?;

    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let server_cfg = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(
            vec![leaf.der().clone(), ca.der().clone()],
            rustls::pki_types::PrivateKeyDer::Pkcs8(leaf_key.serialize_der().into()),
        )?;
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server_cfg));
    let l = TcpListener::bind("127.0.0.1:0").await?;
    let addr = l.local_addr()?;
    tokio::spawn(async move {
        while let Ok((s, _)) = l.accept().await {
            let a = acceptor.clone();
            tokio::spawn(async move {
                if let Ok(mut tls) = a.accept(s).await {
                    let _ = tls.write_all(b"hi").await;
                }
            });
        }
    });

    // Trusted when our CA is in the root store…
    let mut roots = rustls::RootCertStore::empty();
    roots.add(ca.der().clone())?;
    let info = ambar_recon::tls::inspect(
        "localhost",
        addr.port(),
        Some(addr),
        Some(roots),
        Duration::from_secs(5),
    )
    .await?;
    assert!(info.trusted, "{:?}", info.trust_error);
    assert_eq!(info.names, ["localhost"]);
    assert!(info.protocol.contains("1.3"), "{}", info.protocol);
    assert!((9..=10).contains(&info.days_left), "{}", info.days_left);
    assert!(
        info.problems.iter().any(|p| p.contains("expires in")),
        "{:?}",
        info.problems
    );
    assert!(!info.self_signed);
    // …and not trusted by a client that doesn't know the CA.
    let info = ambar_recon::tls::inspect(
        "localhost",
        addr.port(),
        Some(addr),
        Some(rustls::RootCertStore::empty()),
        Duration::from_secs(5),
    )
    .await?;
    assert!(!info.trusted && info.problems.iter().any(|p| p.contains("not trusted")));
    // A name the certificate doesn't cover is not trusted either.
    let mut roots = rustls::RootCertStore::empty();
    roots.add(ca.der().clone())?;
    let info = ambar_recon::tls::inspect(
        "other.test",
        addr.port(),
        Some(addr),
        Some(roots),
        Duration::from_secs(5),
    )
    .await?;
    assert!(!info.trusted, "wrong name must not be trusted");
    Ok(())
}

fn time_in_days(days: i64) -> time::OffsetDateTime {
    time::OffsetDateTime::now_utc() + time::Duration::days(days)
}
