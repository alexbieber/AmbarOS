//! Subdomain discovery: certificate-transparency logs (passive) and DNS guesses (active).

use std::collections::BTreeMap;
use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

/// Common first labels; enough to find the usual hosts without being noisy.
pub const WORDS: &[&str] = &[
    "www",
    "mail",
    "webmail",
    "smtp",
    "imap",
    "pop",
    "mx",
    "ns1",
    "ns2",
    "dns",
    "vpn",
    "remote",
    "portal",
    "admin",
    "api",
    "app",
    "apps",
    "dev",
    "staging",
    "stage",
    "test",
    "qa",
    "uat",
    "beta",
    "demo",
    "preview",
    "sandbox",
    "intranet",
    "internal",
    "corp",
    "sso",
    "auth",
    "login",
    "id",
    "accounts",
    "git",
    "gitlab",
    "github",
    "jenkins",
    "ci",
    "cd",
    "build",
    "jira",
    "confluence",
    "wiki",
    "docs",
    "help",
    "support",
    "status",
    "monitor",
    "grafana",
    "kibana",
    "elastic",
    "prometheus",
    "metrics",
    "logs",
    "cdn",
    "static",
    "assets",
    "img",
    "images",
    "media",
    "files",
    "download",
    "uploads",
    "backup",
    "db",
    "mysql",
    "postgres",
    "redis",
    "mongo",
    "s3",
    "storage",
    "shop",
    "store",
    "pay",
    "payments",
    "billing",
    "crm",
    "erp",
    "hr",
    "m",
    "mobile",
    "blog",
    "news",
    "forum",
    "community",
    "events",
    "careers",
    "partners",
    "cloud",
    "k8s",
    "kube",
    "proxy",
    "gateway",
    "edge",
    "ftp",
    "sftp",
    "owa",
    "exchange",
    "autodiscover",
    "office",
    "teams",
    "chat",
    "meet",
    "calendar",
    "old",
    "new",
    "v1",
    "v2",
];

pub type Resolver =
    Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = Vec<IpAddr>> + Send>> + Send + Sync>;

/// The system resolver (what every other program on this computer uses).
pub fn system_resolver() -> Resolver {
    Arc::new(|name: String| {
        Box::pin(async move {
            tokio::net::lookup_host((name.as_str(), 0))
                .await
                .map(|it| it.map(|a| a.ip()).collect())
                .unwrap_or_default()
        })
    })
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Found {
    pub name: String,
    pub addresses: Vec<IpAddr>,
    /// "ct" (certificate transparency), "dns" (guessed), or both.
    pub sources: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CtEntry {
    name_value: String,
}

/// Names from certificate-transparency logs (crt.sh). Nothing touches the target.
pub async fn from_ct(client: &reqwest::Client, base: &str, domain: &str) -> Result<Vec<String>> {
    let url = format!("{base}/?q=%25.{domain}&output=json");
    let entries: Vec<CtEntry> = client
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mut names: Vec<String> = entries
        .iter()
        .flat_map(|e| e.name_value.lines())
        .map(|n| n.trim().trim_start_matches("*.").to_ascii_lowercase())
        .filter(|n| n == domain || n.ends_with(&format!(".{domain}")))
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

pub async fn discover(
    domain: &str,
    words: &[String],
    ct_names: Vec<String>,
    resolve: Resolver,
    concurrency: usize,
) -> Vec<Found> {
    // A wildcard DNS record answers for every name; don't report guesses then.
    let probe = format!("ambar-{}.{domain}", std::process::id() ^ 0x5eed);
    let wildcard = !resolve(probe).await.is_empty();
    let mut wanted: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for n in ct_names {
        wanted.entry(n).or_default().push("ct".into());
    }
    if !wildcard {
        for w in words {
            wanted
                .entry(format!("{w}.{domain}"))
                .or_default()
                .push("dns".into());
        }
    }
    let limit = Arc::new(Semaphore::new(concurrency.max(1)));
    let mut set = JoinSet::new();
    for (name, sources) in wanted {
        let resolve = resolve.clone();
        let limit = limit.clone();
        set.spawn(async move {
            let _p = limit.acquire_owned().await.ok()?;
            let addrs = resolve(name.clone()).await;
            // CT names are reported even if they no longer resolve; guesses only when they do.
            (!addrs.is_empty() || sources.iter().any(|s| s == "ct")).then_some(Found {
                name,
                addresses: addrs,
                sources,
            })
        });
    }
    let mut out = Vec::new();
    while let Some(r) = set.join_next().await {
        if let Ok(Some(f)) = r {
            out.push(f);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(known: &'static [(&'static str, &'static str)]) -> Resolver {
        Arc::new(move |name: String| {
            Box::pin(async move {
                known
                    .iter()
                    .filter(|(n, _)| *n == name)
                    .filter_map(|(_, ip)| ip.parse().ok())
                    .collect()
            })
        })
    }

    #[tokio::test]
    async fn guesses_that_resolve_are_reported() {
        let r = fake(&[
            ("www.acme.example", "192.0.2.10"),
            ("api.acme.example", "192.0.2.11"),
        ]);
        let words: Vec<String> = ["www", "api", "nope"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let found = discover(
            "acme.example",
            &words,
            vec!["old.acme.example".into(), "www.acme.example".into()],
            r,
            8,
        )
        .await;
        let names: Vec<_> = found.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            ["api.acme.example", "old.acme.example", "www.acme.example"]
        );
        let www = found
            .iter()
            .find(|f| f.name == "www.acme.example")
            .map(|f| f.sources.clone())
            .unwrap_or_default();
        assert_eq!(www, ["ct", "dns"]);
    }

    #[tokio::test]
    async fn wildcard_dns_suppresses_guesses() {
        let r: Resolver = Arc::new(|_name: String| {
            Box::pin(async { vec!["192.0.2.1".parse().unwrap_or(IpAddr::from([0, 0, 0, 0]))] })
        });
        let words: Vec<String> = vec!["www".into(), "api".into()];
        let found = discover("acme.example", &words, vec![], r, 4).await;
        assert!(found.is_empty(), "{found:?}");
    }
}
