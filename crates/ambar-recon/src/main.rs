//! `ambar-recon`: ports, services, HTTP, TLS and subdomains, only inside the engagement scope.

use std::io::Write;
use std::net::IpAddr;
use std::path::PathBuf;
use std::time::Duration;

use ambar_recon::scope::{Scope, Target};
use ambar_recon::{http, ports, subdomains, tls};
use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use clap::{Parser, Subcommand};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(
    name = "ambar-recon",
    version,
    about = "Ambar's reconnaissance scanner. Scans only targets in your engagement's scope."
)]
struct Cli {
    /// Engagement scope file. Without one, only this computer (localhost) can be scanned.
    #[arg(long, global = true, env = "AMBAR_SCOPE")]
    scope: Option<PathBuf>,
    /// One JSON object per line instead of a table.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Find open TCP ports and identify their services.
    Ports {
        targets: Vec<String>,
        /// "top" (default), "all", "22,80,443" or "1-1024".
        #[arg(short, long, default_value = "top")]
        ports: String,
        #[arg(long, default_value_t = 256)]
        concurrency: usize,
        #[arg(long, default_value_t = 1500)]
        timeout_ms: u64,
        /// Don't read service banners.
        #[arg(long)]
        no_banners: bool,
    },
    /// Status, title, server, technologies and missing security headers.
    Http {
        targets: Vec<String>,
        #[arg(long, default_value_t = 10)]
        timeout: u64,
    },
    /// TLS version, cipher and certificate (expiry, names, trust).
    Tls { targets: Vec<String> },
    /// Find subdomains from certificate-transparency logs and DNS guesses.
    Subdomains {
        domain: String,
        /// Skip certificate-transparency logs (crt.sh).
        #[arg(long)]
        no_ct: bool,
        /// Skip DNS guesses.
        #[arg(long)]
        no_guess: bool,
        /// Your own word list for guesses (one label per line).
        #[arg(long)]
        wordlist: Option<PathBuf>,
    },
    /// Check whether targets are in scope, without touching them.
    Scope { targets: Vec<String> },
}

#[derive(Serialize)]
struct Refused<'a> {
    refused: &'a str,
    reason: String,
}

fn today() -> NaiveDate {
    std::env::var("AMBAR_RECON_TODAY")
        .ok()
        .and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok())
        .unwrap_or_else(|| chrono::Local::now().date_naive())
}

struct Out {
    json: bool,
}

impl Out {
    fn item<T: Serialize>(&self, v: &T, human: impl FnOnce() -> String) {
        let line = if self.json {
            serde_json::to_string(v).unwrap_or_default()
        } else {
            human()
        };
        let _ = writeln!(std::io::stdout(), "{line}");
    }
    fn refused(&self, target: &str, reason: &str) {
        self.item(
            &Refused {
                refused: target,
                reason: reason.into(),
            },
            || format!("✗ {target}: {reason}"),
        );
    }
}

/// Targets that passed the scope check; refusals are printed.
fn allowed(scope: &Scope, out: &Out, targets: &[String]) -> Vec<(String, Target)> {
    let day = today();
    targets
        .iter()
        .filter_map(
            |raw| match Target::parse(raw).and_then(|t| scope.check(&t, day).map(|_| t)) {
                Ok(t) => Some((raw.clone(), t)),
                Err(e) => {
                    out.refused(raw, &e.to_string());
                    None
                }
            },
        )
        .collect()
}

async fn addresses(scope: &Scope, out: &Out, raw: &str, t: &Target) -> Vec<IpAddr> {
    match t {
        Target::Ip(ip) => vec![*ip],
        Target::Host(h) => {
            let ips: Vec<IpAddr> = tokio::net::lookup_host((h.as_str(), 0))
                .await
                .map(|it| it.map(|a| a.ip()).collect())
                .unwrap_or_default();
            if ips.is_empty() {
                out.refused(raw, "doesn't resolve");
            }
            let mut ok = Vec::new();
            for ip in ips {
                match scope.check_resolved(t, ip) {
                    Ok(()) if !ok.contains(&ip) => ok.push(ip),
                    Ok(()) => {}
                    Err(e) => out.refused(raw, &e.to_string()),
                }
            }
            ok
        }
    }
}

fn log_activity(path: Option<&PathBuf>, args: &[String]) {
    let Some(dir) = path.and_then(|p| p.parent()) else {
        return;
    };
    let line = format!(
        "{} ambar-recon {}\n",
        chrono::Local::now().to_rfc3339(),
        args.join(" ")
    );
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("activity.log"))
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let scope = match &cli.scope {
        Some(p) => Scope::load(p)?,
        None => Scope::self_only(),
    };
    let out = Out { json: cli.json };
    // Scope checks touch nothing, so only real scans go in the engagement's log.
    if !matches!(cli.cmd, Cmd::Scope { .. }) {
        log_activity(
            cli.scope.as_ref(),
            &std::env::args().skip(1).collect::<Vec<_>>(),
        );
    }
    match cli.cmd {
        Cmd::Scope { targets } => {
            for (raw, _) in allowed(&scope, &out, &targets) {
                out.item(&serde_json::json!({"in_scope": raw}), || {
                    format!("✓ {raw} is in scope")
                });
            }
        }
        Cmd::Ports {
            targets,
            ports: spec,
            concurrency,
            timeout_ms,
            no_banners,
        } => {
            if targets.is_empty() {
                bail!("give at least one target");
            }
            let list = ports::parse_ports(&spec)?;
            let opt = ports::Options {
                concurrency,
                connect_timeout: Duration::from_millis(timeout_ms),
                banners: !no_banners,
                ..Default::default()
            };
            for (raw, t) in allowed(&scope, &out, &targets) {
                let ips = addresses(&scope, &out, &raw, &t).await;
                if ips.is_empty() {
                    continue;
                }
                if !cli.json {
                    println!("{raw} ({} ports)", list.len());
                }
                let open = ports::scan(&ips, &list, &opt).await;
                if open.is_empty() && !cli.json {
                    println!("  no open ports");
                }
                for p in &open {
                    out.item(p, || {
                        format!(
                            "  {:<15} {:>5}/tcp  {:<14} {}",
                            p.ip,
                            p.port,
                            p.service,
                            p.banner.as_deref().unwrap_or("")
                        )
                    });
                }
            }
        }
        Cmd::Http { targets, timeout } => {
            let client = http::client(Duration::from_secs(timeout))?;
            for (raw, _) in allowed(&scope, &out, &targets) {
                let url = if raw.contains("://") {
                    raw.clone()
                } else {
                    format!("https://{raw}")
                };
                let res = match http::fingerprint(&client, &url).await {
                    Ok(r) => Ok(r),
                    Err(_) if !raw.contains("://") => {
                        http::fingerprint(&client, &format!("http://{raw}")).await
                    }
                    Err(e) => Err(e),
                };
                // A redirect may leave the scope: say so instead of quietly reporting it.
                match res {
                    Ok(info) => {
                        if let Ok(t) = Target::parse(&info.final_url)
                            && scope.check(&t, today()).is_err()
                        {
                            out.refused(
                                &info.final_url,
                                "redirected outside the scope; not followed further",
                            );
                        }
                        out.item(&info, || {
                            format!(
                                "{} → {} {}\n  title: {}\n  server: {}\n  tech: {}\n  missing headers: {}{}",
                                info.url,
                                info.status,
                                info.final_url,
                                info.title.as_deref().unwrap_or("-"),
                                info.server.as_deref().unwrap_or("-"),
                                if info.technologies.is_empty() { "-".into() } else { info.technologies.join(", ") },
                                if info.missing_security_headers.is_empty() { "none".into() } else { info.missing_security_headers.join(", ") },
                                if info.weak_cookies.is_empty() { String::new() } else { format!("\n  cookies without HttpOnly/Secure: {}", info.weak_cookies.join(", ")) }
                            )
                        })
                    }
                    Err(e) => out.refused(&raw, &format!("no HTTP answer: {e}")),
                }
            }
        }
        Cmd::Tls { targets } => {
            for (raw, t) in allowed(&scope, &out, &targets) {
                let port = raw
                    .rsplit_once(':')
                    .and_then(|(_, p)| p.trim_end_matches(']').parse::<u16>().ok())
                    .filter(|_| !raw.ends_with(']'))
                    .unwrap_or(443);
                let host = t.to_string();
                let ips = addresses(&scope, &out, &raw, &t).await;
                let Some(ip) = ips.first() else { continue };
                match tls::inspect(&host, port, Some((*ip, port).into()), None, Duration::from_secs(10)).await {
                    Ok(i) => out.item(&i, || {
                        format!(
                            "{}  {} {}\n  subject: {}\n  issuer: {}\n  names: {}\n  expires: {} ({} days)\n  trusted: {}{}",
                            i.target, i.protocol, i.cipher, i.subject, i.issuer, i.names.join(", "), i.not_after, i.days_left,
                            if i.trusted { "yes" } else { "no" },
                            i.problems.iter().map(|p| format!("\n  ! {p}")).collect::<String>()
                        )
                    }),
                    Err(e) => out.refused(&raw, &format!("TLS failed: {e:#}")),
                }
            }
        }
        Cmd::Subdomains {
            domain,
            no_ct,
            no_guess,
            wordlist,
        } => {
            let target = Target::parse(&domain)?;
            let domain = target.to_string();
            // The domain itself, or its subdomains, must be in scope.
            let probe = Target::Host(format!("ambar-scope-probe.{domain}"));
            if scope.check(&target, today()).is_err() && scope.check(&probe, today()).is_err() {
                out.refused(
                    &domain,
                    "neither the domain nor its subdomains are in scope",
                );
                return Ok(());
            }
            let client = http::client(Duration::from_secs(30))?;
            let ct = if no_ct {
                Vec::new()
            } else {
                let base =
                    std::env::var("AMBAR_RECON_CT_URL").unwrap_or_else(|_| "https://crt.sh".into());
                subdomains::from_ct(&client, &base, &domain)
                    .await
                    .unwrap_or_else(|e| {
                        eprintln!("certificate-transparency lookup failed: {e}");
                        Vec::new()
                    })
            };
            let words: Vec<String> = match (&wordlist, no_guess) {
                (_, true) => Vec::new(),
                (Some(p), _) => std::fs::read_to_string(p)
                    .with_context(|| format!("reading {}", p.display()))?
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty() && !l.starts_with('#'))
                    .map(str::to_string)
                    .collect(),
                (None, _) => subdomains::WORDS.iter().map(|s| s.to_string()).collect(),
            };
            for f in
                subdomains::discover(&domain, &words, ct, subdomains::system_resolver(), 64).await
            {
                let in_scope = Target::parse(&f.name)
                    .map(|t| scope.check(&t, today()).is_ok())
                    .unwrap_or(false);
                let v = serde_json::json!({"name": f.name, "addresses": f.addresses, "sources": f.sources, "in_scope": in_scope});
                out.item(&v, || {
                    let ips: Vec<String> = f.addresses.iter().map(|a| a.to_string()).collect();
                    format!(
                        "{:<40} {:<32} {}{}",
                        f.name,
                        if ips.is_empty() {
                            "(no address)".into()
                        } else {
                            ips.join(" ")
                        },
                        f.sources.join("+"),
                        if in_scope { "" } else { "  [out of scope]" }
                    )
                });
            }
        }
    }
    Ok(())
}
