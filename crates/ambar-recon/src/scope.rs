//! Engagement scope: which targets a scan may touch.
//!
//! Every Ambar security tool reads the same `scope.toml`:
//!
//! ```toml
//! [engagement]
//! name = "ACME external test"
//! authorised_by = "Jane Doe, CISO, signed 2026-11-10"
//! starts = "2026-11-12"
//! ends = "2026-11-30"
//!
//! [scope]
//! include = ["203.0.113.0/24", "acme.example", "*.acme.example"]
//! exclude = ["203.0.113.1", "vpn.acme.example"]
//! ```
//!
//! A target is allowed only if it matches an `include` entry, matches no `exclude`
//! entry, and today is within the engagement dates. Your own machine (loopback)
//! is always allowed, with or without a scope file.

use std::net::IpAddr;
use std::path::Path;

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use ipnet::IpNet;
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Engagement {
    pub name: Option<String>,
    pub authorised_by: Option<String>,
    #[serde(default, deserialize_with = "date")]
    pub starts: Option<NaiveDate>,
    #[serde(default, deserialize_with = "date")]
    pub ends: Option<NaiveDate>,
}

/// Dates may be written as TOML dates (`ends = 2026-11-30`) or strings (`ends = "2026-11-30"`).
fn date<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<NaiveDate>, D::Error> {
    use serde::de::Error;
    let text = match Option::<toml::Value>::deserialize(d)? {
        None => return Ok(None),
        Some(toml::Value::String(s)) => s,
        Some(toml::Value::Datetime(dt)) => dt.to_string(),
        Some(other) => {
            return Err(D::Error::custom(format!(
                "expected a date like 2026-11-30, got {other}"
            )));
        }
    };
    NaiveDate::parse_from_str(text.get(..10).unwrap_or(&text), "%Y-%m-%d")
        .map(Some)
        .map_err(|_| D::Error::custom(format!("expected a date like 2026-11-30, got {text}")))
}

#[derive(Debug, Clone, Default, Deserialize)]
struct Lists {
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct File {
    #[serde(default)]
    engagement: Engagement,
    #[serde(default)]
    scope: Lists,
}

/// One scope entry: a network, an exact host name, or a `*.domain` wildcard.
#[derive(Debug, Clone, PartialEq)]
enum Rule {
    Net(IpNet),
    Host(String),
    Wildcard(String),
}

impl Rule {
    fn parse(s: &str) -> Result<Rule> {
        let s = s.trim().trim_end_matches('.').to_ascii_lowercase();
        if s.is_empty() {
            bail!("empty scope entry");
        }
        if let Ok(net) = s.parse::<IpNet>() {
            return Ok(Rule::Net(net.trunc()));
        }
        if let Ok(ip) = s.parse::<IpAddr>() {
            return Ok(Rule::Net(IpNet::from(ip)));
        }
        if let Some(rest) = s.strip_prefix("*.") {
            valid_host(rest)?;
            return Ok(Rule::Wildcard(rest.to_string()));
        }
        valid_host(&s)?;
        Ok(Rule::Host(s))
    }

    fn matches(&self, t: &Target) -> bool {
        match (self, t) {
            (Rule::Net(n), Target::Ip(ip)) => n.contains(ip),
            (Rule::Host(h), Target::Host(name)) => h == name,
            // *.example.com covers sub.example.com and deeper, not example.com itself
            (Rule::Wildcard(d), Target::Host(name)) => name.ends_with(&format!(".{d}")),
            _ => false,
        }
    }
}

fn valid_host(s: &str) -> Result<()> {
    let ok = !s.is_empty()
        && s.len() <= 253
        && s.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                && !l.starts_with('-')
                && !l.ends_with('-')
        });
    if ok {
        Ok(())
    } else {
        bail!("not a valid host name or network: {s}")
    }
}

/// What a command was asked to touch.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Ip(IpAddr),
    Host(String),
}

impl Target {
    pub fn parse(s: &str) -> Result<Target> {
        let s = s.trim();
        // Accept URLs and host:port, scope is about the host.
        let s = s.split("://").last().unwrap_or(s);
        let s = s.split(['/', '?', '#']).next().unwrap_or(s);
        let host = if let Some(inner) = s.strip_prefix('[') {
            inner.split(']').next().unwrap_or(inner)
        } else if s.matches(':').count() == 1 {
            s.split(':').next().unwrap_or(s)
        } else {
            s
        };
        if let Ok(ip) = host.parse::<IpAddr>() {
            return Ok(Target::Ip(ip));
        }
        let h = host.trim_end_matches('.').to_ascii_lowercase();
        valid_host(&h)?;
        Ok(Target::Host(h))
    }

    pub fn is_self(&self) -> bool {
        match self {
            Target::Ip(ip) => ip.is_loopback(),
            Target::Host(h) => h == "localhost",
        }
    }
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Target::Ip(ip) => write!(f, "{ip}"),
            Target::Host(h) => write!(f, "{h}"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Scope {
    pub engagement: Engagement,
    include: Vec<Rule>,
    exclude: Vec<Rule>,
    /// No scope file: only this computer may be scanned.
    self_only: bool,
}

impl Scope {
    pub fn self_only() -> Scope {
        Scope {
            self_only: true,
            ..Scope::default()
        }
    }

    pub fn load(path: &Path) -> Result<Scope> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading scope file {}", path.display()))?;
        Scope::parse(&text).with_context(|| format!("in {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Scope> {
        let f: File = toml::from_str(text)?;
        if f.scope.include.is_empty() {
            bail!("[scope] include is empty: list the networks and domains you're allowed to test");
        }
        let parse = |v: &[String]| v.iter().map(|s| Rule::parse(s)).collect::<Result<Vec<_>>>();
        Ok(Scope {
            engagement: f.engagement,
            include: parse(&f.scope.include)?,
            exclude: parse(&f.scope.exclude)?,
            self_only: false,
        })
    }

    /// Ok if `target` may be touched today; the error says why not.
    pub fn check(&self, target: &Target, today: NaiveDate) -> Result<()> {
        if target.is_self() {
            return Ok(());
        }
        if self.self_only {
            bail!(
                "{target} is not this computer. Scanning other systems needs a scope file (--scope) for an authorised engagement"
            );
        }
        if let Some(start) = self.engagement.starts
            && today < start
        {
            bail!("the engagement starts on {start}; {target} can't be scanned before then");
        }
        if let Some(end) = self.engagement.ends
            && today > end
        {
            bail!("the engagement ended on {end}; {target} is no longer in scope");
        }
        if self.exclude.iter().any(|r| r.matches(target)) {
            bail!("{target} is explicitly excluded from the scope");
        }
        if !self.include.iter().any(|r| r.matches(target)) {
            bail!("{target} is outside the scope");
        }
        Ok(())
    }

    /// Resolved addresses of an in-scope host name must not land on an excluded network.
    pub fn check_resolved(&self, host: &Target, ip: IpAddr) -> Result<()> {
        if self.exclude.iter().any(|r| r.matches(&Target::Ip(ip))) {
            bail!("{host} resolves to {ip}, which is excluded from the scope");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap_or_default()
    }

    const FILE: &str = r#"
        [engagement]
        name = "test"
        starts = "2026-11-01"
        ends = "2026-11-30"
        [scope]
        include = ["10.0.0.0/24", "acme.example", "*.acme.example", "2001:db8::/64"]
        exclude = ["10.0.0.1", "vpn.acme.example"]
    "#;

    fn t(s: &str) -> Target {
        Target::parse(s).unwrap_or(Target::Host("invalid".into()))
    }

    #[test]
    fn includes_excludes_and_wildcards() -> Result<()> {
        let s = Scope::parse(FILE)?;
        let d = day("2026-11-12");
        assert!(s.check(&t("10.0.0.7"), d).is_ok());
        assert!(s.check(&t("10.0.1.7"), d).is_err());
        assert!(s.check(&t("10.0.0.1"), d).is_err(), "excluded host");
        assert!(s.check(&t("acme.example"), d).is_ok());
        assert!(s.check(&t("www.acme.example"), d).is_ok());
        assert!(s.check(&t("a.b.acme.example"), d).is_ok());
        assert!(s.check(&t("vpn.acme.example"), d).is_err(), "excluded name");
        assert!(
            s.check(&t("evilacme.example"), d).is_err(),
            "suffix without a dot is not a subdomain"
        );
        assert!(s.check(&t("acme.example.evil.com"), d).is_err());
        assert!(s.check(&t("2001:db8::5"), d).is_ok());
        assert!(s.check(&t("2001:db9::5"), d).is_err());
        Ok(())
    }

    #[test]
    fn urls_and_ports_are_reduced_to_the_host() {
        assert_eq!(
            t("https://www.acme.example:8443/login?x=1"),
            Target::Host("www.acme.example".into())
        );
        assert_eq!(
            t("10.0.0.7:22"),
            Target::Ip("10.0.0.7".parse().unwrap_or(IpAddr::from([0, 0, 0, 0])))
        );
        assert_eq!(
            t("[2001:db8::5]:443"),
            Target::Ip("2001:db8::5".parse().unwrap_or(IpAddr::from([0, 0, 0, 0])))
        );
        assert_eq!(
            t("WWW.Acme.Example."),
            Target::Host("www.acme.example".into())
        );
    }

    #[test]
    fn toml_dates_and_string_dates_both_work() -> Result<()> {
        let s = Scope::parse(
            "[engagement]\nstarts = 2026-11-01\nends = \"2026-11-30\"\n[scope]\ninclude = [\"10.0.0.0/8\"]",
        )?;
        assert_eq!(s.engagement.starts, Some(day("2026-11-01")));
        assert_eq!(s.engagement.ends, Some(day("2026-11-30")));
        assert!(
            Scope::parse("[engagement]\nends = 5\n[scope]\ninclude = [\"10.0.0.0/8\"]").is_err()
        );
        Ok(())
    }

    #[test]
    fn dates_are_enforced() -> Result<()> {
        let s = Scope::parse(FILE)?;
        assert!(s.check(&t("10.0.0.7"), day("2026-10-31")).is_err());
        assert!(s.check(&t("10.0.0.7"), day("2026-12-01")).is_err());
        assert!(s.check(&t("10.0.0.7"), day("2026-11-30")).is_ok());
        Ok(())
    }

    #[test]
    fn without_a_scope_only_this_computer() {
        let s = Scope::self_only();
        let d = day("2026-11-12");
        assert!(s.check(&t("127.0.0.1"), d).is_ok());
        assert!(s.check(&t("localhost"), d).is_ok());
        assert!(s.check(&t("::1"), d).is_ok());
        let err = s
            .check(&t("192.168.1.10"), d)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(err.contains("scope file"), "{err}");
    }

    #[test]
    fn bad_files_are_rejected() {
        assert!(Scope::parse("[scope]\ninclude = []").is_err());
        assert!(Scope::parse("[scope]\ninclude = [\"exa mple.com\"]").is_err());
        assert!(Scope::parse("[scope]\ninclude = [\"-bad.com\"]").is_err());
        assert!(Target::parse("not a host!").is_err());
    }

    #[test]
    fn resolved_addresses_cannot_reach_excluded_networks() -> Result<()> {
        let s = Scope::parse(FILE)?;
        let host = t("www.acme.example");
        assert!(s.check_resolved(&host, "10.0.0.1".parse()?).is_err());
        assert!(s.check_resolved(&host, "10.0.0.9".parse()?).is_ok());
        Ok(())
    }
}
