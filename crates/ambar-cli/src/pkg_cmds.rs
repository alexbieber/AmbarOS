//! `ambar pkg …`: build, sign, share and install workflow packages.

use std::path::{Path, PathBuf};

use ambar_automation::RiskLevel;
use ambar_pkg::{
    Index, Installed, Package, PkgError, PublicKeyFile, Publisher, Registry, TrustStore, Verified,
};
use ambar_service::protocol::GrantSpec;
use ambar_service::{Client, ServicePaths};
use anyhow::{Context, Result, bail};
use clap::Subcommand;
use serde::{Deserialize, Serialize};

#[derive(Subcommand, Debug)]
pub enum PkgCmd {
    /// Create a publisher signing key (kept in ~/.config/ambar/keys, mode 0600).
    Keygen {
        /// Publisher name shown to people who install your packages.
        name: String,
    },
    /// Build and sign a package from a directory containing ambar-package.toml.
    Build {
        /// Source directory.
        dir: PathBuf,
        /// Signing key: a path, or a name under ~/.config/ambar/keys.
        #[arg(long)]
        key: String,
        /// Output file (default: <name>-<version>.ambarpkg).
        #[arg(long, short)]
        output: Option<PathBuf>,
    },
    /// Verify a package file and show what it contains.
    Verify {
        /// Package file.
        file: PathBuf,
    },
    /// Trust a publisher's public key (.pub file).
    Trust {
        /// Public key file.
        public_key: PathBuf,
    },
    /// Stop trusting a publisher.
    Untrust {
        /// Key fingerprint.
        fingerprint: String,
    },
    /// List trusted publishers.
    Publishers,
    /// Add a package to a marketplace index (creating the index if needed).
    Publish {
        /// Package file.
        file: PathBuf,
        /// Index file (index.json); the package is copied next to it.
        #[arg(long)]
        index: PathBuf,
    },
    /// Search the marketplace.
    Search {
        /// Search term (empty lists everything).
        #[arg(default_value = "")]
        term: String,
        /// Index location (path or https URL). Default: ~/.config/ambar/marketplace.toml.
        #[arg(long)]
        index: Option<String>,
    },
    /// Install a package from a file or from the marketplace by name.
    Install {
        /// A .ambarpkg file, or a package name from the index.
        package: String,
        /// Index location for name lookups.
        #[arg(long)]
        index: Option<String>,
        /// Trust the publisher if not yet trusted (prints the fingerprint).
        #[arg(long)]
        trust: bool,
        /// Approve the permissions without asking (refused for high-risk packages).
        #[arg(long)]
        yes: bool,
        /// Enable the workflows after installing.
        #[arg(long)]
        enable: bool,
        /// Pre-approve sensitive actions in these workflows.
        #[arg(long)]
        allow_sensitive: bool,
    },
    /// List installed packages.
    List,
    /// Remove an installed package and its workflows.
    Remove {
        /// Package name.
        name: String,
    },
}

fn config_dir() -> Result<PathBuf> {
    if let Some(d) = std::env::var_os("AMBAR_CONFIG_DIR") {
        return Ok(PathBuf::from(d));
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .context("HOME is not set")?;
    Ok(base.join("ambar"))
}

fn trust_store() -> Result<TrustStore> {
    Ok(TrustStore::open(config_dir()?.join("trusted-keys")))
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct MarketConfig {
    index: Option<String>,
}

fn index_source(explicit: &Option<String>) -> Result<String> {
    if let Some(i) = explicit {
        return Ok(i.clone());
    }
    let p = config_dir()?.join("marketplace.toml");
    let cfg: MarketConfig = std::fs::read_to_string(&p)
        .ok()
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or_default();
    cfg.index.with_context(|| {
        format!(
            "no marketplace configured: pass --index or set `index = \"…\"` in {}",
            p.display()
        )
    })
}

async fn fetch(src: &str) -> Result<Vec<u8>> {
    if let Some(rest) = src.strip_prefix("http://") {
        let host = rest.split(['/', ':']).next().unwrap_or("");
        if !matches!(host, "localhost" | "127.0.0.1") {
            bail!("refusing plain-HTTP download {src}; use https://");
        }
    }
    if src.starts_with("https://") || src.starts_with("http://") {
        let resp = reqwest::get(src)
            .await
            .with_context(|| format!("downloading {src}"))?;
        if !resp.status().is_success() {
            bail!("{src} returned {}", resp.status());
        }
        let bytes = resp.bytes().await?;
        if bytes.len() > ambar_pkg::MAX_PACKAGE_BYTES * 2 {
            bail!("{src} is too large");
        }
        return Ok(bytes.to_vec());
    }
    std::fs::read(src).with_context(|| format!("reading {src}"))
}

/// Resolves `url` relative to the index location.
fn resolve(index_src: &str, url: &str) -> String {
    if url.starts_with("https://") || url.starts_with("http://") || url.starts_with('/') {
        return url.to_string();
    }
    match index_src.rfind('/') {
        Some(i) => format!("{}/{url}", &index_src[..i]),
        None => url.to_string(),
    }
}

fn key_path(key: &str) -> Result<PathBuf> {
    let p = PathBuf::from(key);
    if p.exists() {
        return Ok(p);
    }
    Ok(config_dir()?.join("keys").join(format!("{key}.key")))
}

fn show(v: &Verified, trusted: bool) {
    let m = &v.package.manifest;
    println!("{} {} — {}", m.name, m.version, m.description);
    println!(
        "  publisher  {} ({}){}",
        v.package.signature.publisher,
        v.fingerprint,
        if trusted {
            " — trusted"
        } else {
            " — NOT trusted"
        }
    );
    if !m.license.is_empty() {
        println!("  license    {}", m.license);
    }
    println!("  signature  valid · sha256 {}", &v.sha256[..16]);
    println!();
    for wf in &v.workflows {
        crate::describe(wf);
        for r in ambar_automation::risk::review(wf) {
            let tag = match r.level {
                RiskLevel::High => "HIGH",
                RiskLevel::Warning => "warning",
                RiskLevel::Info => "info",
            };
            println!("  {tag}: {}", r.message);
        }
        println!();
    }
}

pub async fn pkg(cmd: &PkgCmd, paths: &ServicePaths) -> Result<i32> {
    match cmd {
        PkgCmd::Keygen { name } => {
            let stem: String = name
                .to_lowercase()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect();
            let dir = config_dir()?.join("keys");
            if dir.join(format!("{stem}.key")).exists() {
                bail!("a key named {stem} already exists in {}", dir.display());
            }
            let p = Publisher::generate(name)?;
            let (k, pubf) = p.save(&dir, &stem)?;
            println!(
                "created signing key {} (keep it private, back it up)",
                k.display()
            );
            println!(
                "public key {} — share this so people can trust you",
                pubf.display()
            );
            println!("fingerprint {}", p.public().fingerprint());
        }
        PkgCmd::Build { dir, key, output } => {
            let publisher = Publisher::load(&key_path(key)?)?;
            let pkg = Package::build(dir, &publisher)?;
            let m = &pkg.manifest;
            let out = output
                .clone()
                .unwrap_or_else(|| PathBuf::from(format!("{}-{}.ambarpkg", m.name, m.version)));
            let bytes = pkg.to_bytes()?;
            std::fs::write(&out, &bytes)?;
            let v = Package::verify(&bytes)?;
            println!(
                "built {} ({} workflow{}, signed by {} {})",
                out.display(),
                v.workflows.len(),
                if v.workflows.len() == 1 { "" } else { "s" },
                publisher.name,
                v.fingerprint
            );
        }
        PkgCmd::Verify { file } => {
            let v = Package::verify(&std::fs::read(file)?)?;
            let trusted = trust_store()?.require(&v).is_ok();
            show(&v, trusted);
        }
        PkgCmd::Trust { public_key } => {
            let k: PublicKeyFile = serde_json::from_str(&std::fs::read_to_string(public_key)?)
                .context("not an Ambar public key file")?;
            let fp = trust_store()?.trust(&k)?;
            println!("trusted {} ({fp})", k.name);
        }
        PkgCmd::Untrust { fingerprint } => {
            if trust_store()?.untrust(fingerprint) {
                println!("no longer trusting {fingerprint}");
            } else {
                bail!("no trusted key {fingerprint}");
            }
        }
        PkgCmd::Publishers => {
            for k in trust_store()?.list() {
                println!("{}  {}", k.fingerprint(), k.name);
            }
        }
        PkgCmd::Publish { file, index } => {
            let bytes = std::fs::read(file)?;
            let v = Package::verify(&bytes)?;
            let dir = index.parent().map(Path::to_path_buf).unwrap_or_default();
            std::fs::create_dir_all(dir.join("packages"))?;
            let m = &v.package.manifest;
            let rel = format!("packages/{}-{}.ambarpkg", m.name, m.version);
            std::fs::write(dir.join(&rel), &bytes)?;
            let mut idx = match std::fs::read(index) {
                Ok(b) => Index::parse(&b)?,
                Err(_) => Index {
                    format: 1,
                    name: "Ambar marketplace".into(),
                    packages: vec![],
                },
            };
            idx.packages
                .retain(|p| !(p.name == m.name && p.version == m.version));
            idx.packages.push(Index::entry_for(&v, &rel));
            idx.packages
                .sort_by(|a, b| a.name.cmp(&b.name).then(a.version.cmp(&b.version)));
            std::fs::write(index, serde_json::to_string_pretty(&idx)? + "\n")?;
            println!("published {} {} to {}", m.name, m.version, index.display());
        }
        PkgCmd::Search { term, index } => {
            let src = index_source(index)?;
            let idx = Index::parse(&fetch(&src).await?)?;
            let hits = idx.search(term);
            if hits.is_empty() {
                println!("no packages match `{term}` in {}", idx.name);
            }
            let trust = trust_store()?;
            let trusted: Vec<String> = trust
                .list()
                .iter()
                .map(PublicKeyFile::fingerprint)
                .collect();
            for p in hits {
                let mark = if trusted.contains(&p.signer) {
                    "trusted"
                } else {
                    "untrusted"
                };
                println!(
                    "{} {}  — {}\n  by {} · signer {} ({mark})\n",
                    p.name, p.version, p.description, p.author, p.signer
                );
            }
        }
        PkgCmd::Install {
            package,
            index,
            trust,
            yes,
            enable,
            allow_sensitive,
        } => {
            let v = if Path::new(package).is_file() {
                Package::verify(&fetch(package).await?)?
            } else {
                let src = index_source(index)?;
                let idx = Index::parse(&fetch(&src).await?)?;
                let entry = idx
                    .find(package)
                    .with_context(|| format!("`{package}` is not in {}", idx.name))?;
                let bytes = fetch(&resolve(&src, &entry.url)).await?;
                entry.check(&bytes)?
            };
            let store = trust_store()?;
            match store.require(&v) {
                Ok(()) => {}
                Err(PkgError::Untrusted { name, fingerprint }) => {
                    let ok = *trust
                        || crate::ask(&format!(
                            "Publisher \"{name}\" ({fingerprint}) is not trusted. Trust this key? [y/N] "
                        ));
                    if !ok {
                        bail!("not installed: publisher {name} ({fingerprint}) is not trusted");
                    }
                    store.trust(&PublicKeyFile {
                        name: name.clone(),
                        public: v.package.signature.public_key.clone(),
                    })?;
                    println!("now trusting {name} ({fingerprint})");
                }
                Err(e) => return Err(e.into()),
            }
            show(&v, true);
            let high = v.risks().iter().any(|(_, r)| r.level == RiskLevel::High);
            if high && *yes {
                bail!(
                    "this package has high-risk workflows; install it interactively to review them"
                );
            }
            if !*yes && !crate::ask("Install and grant these permissions? [y/N] ") {
                bail!("not installed");
            }
            let mut c = Client::connect(paths, "ambar-pkg")
                .await
                .context("ambard is not running (start it with `ambar service start`)")?;
            let registry = Registry::open(&paths.data_dir);
            let m = &v.package.manifest;
            let upgrading = registry.get(&m.name).is_some();
            let existing: Vec<String> = c.list().await?.into_iter().map(|w| w.id).collect();
            let owned: Vec<String> = registry
                .get(&m.name)
                .map(|i| i.workflows)
                .unwrap_or_default();
            for wf in &v.workflows {
                if existing.contains(&wf.id) && !owned.contains(&wf.id) {
                    bail!(
                        "workflow `{}` already exists and does not belong to {}; remove it first",
                        wf.id,
                        m.name
                    );
                }
            }
            for (rel, wf) in m.workflows.iter().zip(&v.workflows) {
                c.register(
                    &v.package.files[rel],
                    Some(GrantSpec {
                        allow_sensitive: *allow_sensitive,
                    }),
                    existing.contains(&wf.id),
                )
                .await?;
                if *enable {
                    c.enable(&wf.id).await?;
                }
            }
            registry.record(&Installed {
                name: m.name.clone(),
                version: m.version.clone(),
                signer: v.fingerprint.clone(),
                publisher: v.package.signature.publisher.clone(),
                workflows: v.workflows.iter().map(|w| w.id.clone()).collect(),
                installed_at: chrono::Utc::now().to_rfc3339(),
            })?;
            println!(
                "{} {} {}{}",
                if upgrading { "upgraded" } else { "installed" },
                m.name,
                m.version,
                if *enable {
                    " (enabled)"
                } else {
                    " (disabled; enable with `ambar workflow enable <id>`)"
                }
            );
        }
        PkgCmd::List => {
            let list = Registry::open(&paths.data_dir).list();
            if list.is_empty() {
                println!("No packages installed.");
            }
            for i in list {
                println!(
                    "{} {}  by {} ({})  workflows: {}",
                    i.name,
                    i.version,
                    i.publisher,
                    i.signer,
                    i.workflows.join(", ")
                );
            }
        }
        PkgCmd::Remove { name } => {
            let registry = Registry::open(&paths.data_dir);
            let inst = registry
                .get(name)
                .with_context(|| format!("`{name}` is not installed"))?;
            let mut c = Client::connect(paths, "ambar-pkg")
                .await
                .context("ambard is not running (start it with `ambar service start`)")?;
            for id in &inst.workflows {
                match c.remove(id).await {
                    Ok(()) => println!("removed workflow `{id}`"),
                    Err(e) if e.code() == Some(ambar_service::protocol::ErrorCode::NotFound) => {}
                    Err(e) => return Err(e.into()),
                }
            }
            registry.forget(name);
            println!("removed {name}");
        }
    }
    Ok(0)
}
