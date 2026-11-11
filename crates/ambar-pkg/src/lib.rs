//! # ambar-pkg
//!
//! Ambar packages are signed bundles of workflows: the unit of sharing in the
//! Ambar developer ecosystem and marketplace.
//!
//! - A package author writes `ambar-package.toml` plus workflow files, and
//!   builds a `.ambarpkg` signed with their ed25519 key.
//! - Installing verifies the signature and every workflow, requires the
//!   signer to be trusted, and then goes through the *normal* permission
//!   consent for each workflow. A package never grants itself anything.
//! - A marketplace is a JSON index of packages with their SHA-256 and signer
//!   fingerprint; the download is checked against both.
//!
//! Packages contain workflow definitions only. There is deliberately no
//! mechanism for packages to add new action types or run code: that would
//! bypass the closed action set the security model relies on.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use ambar_automation::{ValidationOptions, Workflow, risk};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Package file format version.
pub const FORMAT: u32 = 1;
/// Manifest file name inside a package source directory.
pub const MANIFEST: &str = "ambar-package.toml";
/// Largest accepted package (all files together).
pub const MAX_PACKAGE_BYTES: usize = 1024 * 1024;
/// Most workflows per package.
pub const MAX_WORKFLOWS: usize = 50;

/// Errors.
#[derive(Debug, thiserror::Error)]
pub enum PkgError {
    /// Malformed or inconsistent package.
    #[error("invalid package: {0}")]
    Invalid(String),
    /// Signature or hash check failed.
    #[error("verification failed: {0}")]
    Verification(String),
    /// Signer not in the trust store.
    #[error("untrusted publisher {name} (fingerprint {fingerprint})")]
    Untrusted {
        /// Publisher name claimed by the package.
        name: String,
        /// Key fingerprint.
        fingerprint: String,
    },
    /// I/O failure.
    #[error("{0}: {1}")]
    Io(String, std::io::Error),
}

fn io(ctx: impl Into<String>) -> impl FnOnce(std::io::Error) -> PkgError {
    let c = ctx.into();
    move |e| PkgError::Io(c, e)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex<const N: usize>(s: &str) -> Result<[u8; N], PkgError> {
    let s = s.trim();
    if s.len() != N * 2 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(PkgError::Verification(format!("expected {N} hex bytes")));
    }
    let mut out = [0u8; N];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
            .map_err(|e| PkgError::Verification(e.to_string()))?;
    }
    Ok(out)
}

/// SHA-256 of bytes, hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// Short, human-checkable key fingerprint: first 16 hex chars of SHA-256.
pub fn fingerprint(public_key_hex: &str) -> String {
    sha256_hex(public_key_hex.trim().as_bytes())[..16].to_string()
}

// ------------------------------------------------------------------ keys

/// A publisher's signing key.
#[derive(Debug)]
pub struct Publisher {
    /// Display name.
    pub name: String,
    key: SigningKey,
}

/// Key file contents (`<name>.key`, mode 0600). Never distribute it.
#[derive(Debug, Serialize, Deserialize)]
struct KeyFile {
    name: String,
    secret: String,
    public: String,
}

/// Public key file contents (`<name>.pub`): safe to share.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicKeyFile {
    /// Publisher name.
    pub name: String,
    /// Ed25519 public key, hex.
    pub public: String,
}

impl PublicKeyFile {
    /// Fingerprint of this key.
    pub fn fingerprint(&self) -> String {
        fingerprint(&self.public)
    }
}

impl Publisher {
    /// Generates a new key.
    pub fn generate(name: &str) -> Result<Self, PkgError> {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).map_err(|e| PkgError::Invalid(e.to_string()))?;
        Ok(Self {
            name: name.into(),
            key: SigningKey::from_bytes(&seed),
        })
    }

    /// Public half.
    pub fn public(&self) -> PublicKeyFile {
        PublicKeyFile {
            name: self.name.clone(),
            public: hex(self.key.verifying_key().as_bytes()),
        }
    }

    /// Saves `<dir>/<name>.key` (0600) and `<dir>/<name>.pub`.
    pub fn save(&self, dir: &Path, file_stem: &str) -> Result<(PathBuf, PathBuf), PkgError> {
        std::fs::create_dir_all(dir).map_err(io(dir.display().to_string()))?;
        let key_path = dir.join(format!("{file_stem}.key"));
        let pub_path = dir.join(format!("{file_stem}.pub"));
        let kf = KeyFile {
            name: self.name.clone(),
            secret: hex(&self.key.to_bytes()),
            public: self.public().public,
        };
        let text =
            serde_json::to_string_pretty(&kf).map_err(|e| PkgError::Invalid(e.to_string()))?;
        write_private(&key_path, text.as_bytes())?;
        let pub_text = serde_json::to_string_pretty(&self.public())
            .map_err(|e| PkgError::Invalid(e.to_string()))?;
        std::fs::write(&pub_path, pub_text).map_err(io(pub_path.display().to_string()))?;
        Ok((key_path, pub_path))
    }

    /// Loads a `.key` file.
    pub fn load(path: &Path) -> Result<Self, PkgError> {
        let text = std::fs::read_to_string(path).map_err(io(path.display().to_string()))?;
        let kf: KeyFile =
            serde_json::from_str(&text).map_err(|e| PkgError::Invalid(format!("key file: {e}")))?;
        Ok(Self {
            name: kf.name,
            key: SigningKey::from_bytes(&unhex::<32>(&kf.secret)?),
        })
    }
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), PkgError> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(io(path.display().to_string()))?;
    f.write_all(bytes).map_err(io(path.display().to_string()))
}

// ------------------------------------------------------------------ package

/// `ambar-package.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Reverse-DNS-style id, e.g. `dev.alex.pdf-tools`.
    pub name: String,
    /// `MAJOR.MINOR.PATCH`.
    pub version: String,
    /// One-line description.
    pub description: String,
    /// Author or organisation.
    pub author: String,
    /// SPDX license id.
    #[serde(default)]
    pub license: String,
    /// Project page.
    #[serde(default)]
    pub homepage: String,
    /// Workflow files, relative to the manifest.
    pub workflows: Vec<String>,
}

/// Detached signature block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureBlock {
    /// Publisher name (as claimed; trust is by key, not by name).
    pub publisher: String,
    /// Ed25519 public key, hex.
    pub public_key: String,
    /// Signature over [`Package::signed_bytes`], hex.
    pub signature: String,
}

/// A `.ambarpkg` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    /// File format version.
    pub format: u32,
    /// Manifest.
    pub manifest: Manifest,
    /// File contents by relative path (sorted).
    pub files: BTreeMap<String, String>,
    /// Signature.
    pub signature: SignatureBlock,
}

/// A package whose signature and contents were checked.
#[derive(Debug, Clone)]
pub struct Verified {
    /// The package.
    pub package: Package,
    /// Parsed, validated workflows in manifest order.
    pub workflows: Vec<Workflow>,
    /// Signer key fingerprint.
    pub fingerprint: String,
    /// SHA-256 of the package file.
    pub sha256: String,
}

fn check_name(name: &str) -> Result<(), PkgError> {
    let ok = !name.is_empty()
        && name.len() <= 100
        && name.split('.').count() >= 2
        && name.split('.').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        });
    if ok {
        Ok(())
    } else {
        Err(PkgError::Invalid(format!(
            "package name `{name}` must look like `dev.yourname.package` (lowercase, digits, dashes)"
        )))
    }
}

fn check_version(v: &str) -> Result<(), PkgError> {
    let parts: Vec<&str> = v.split('.').collect();
    if parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.parse::<u64>().is_ok())
    {
        Ok(())
    } else {
        Err(PkgError::Invalid(format!(
            "version `{v}` must be MAJOR.MINOR.PATCH"
        )))
    }
}

fn check_rel_path(p: &str) -> Result<(), PkgError> {
    let path = Path::new(p);
    let ok = !p.is_empty()
        && path.is_relative()
        && path.components().all(|c| matches!(c, Component::Normal(_)))
        && p.ends_with(".toml");
    if ok {
        Ok(())
    } else {
        Err(PkgError::Invalid(format!(
            "`{p}` must be a relative path to a .toml file inside the package"
        )))
    }
}

impl Package {
    /// The exact bytes that are signed: format, manifest and files, as
    /// canonical JSON (struct field order and sorted file map).
    pub fn signed_bytes(&self) -> Result<Vec<u8>, PkgError> {
        serde_json::to_vec(&(self.format, &self.manifest, &self.files))
            .map_err(|e| PkgError::Invalid(e.to_string()))
    }

    /// Builds and signs a package from a source directory.
    pub fn build(dir: &Path, publisher: &Publisher) -> Result<Self, PkgError> {
        let mpath = dir.join(MANIFEST);
        let text = std::fs::read_to_string(&mpath).map_err(io(mpath.display().to_string()))?;
        let manifest: Manifest =
            toml::from_str(&text).map_err(|e| PkgError::Invalid(format!("{MANIFEST}: {e}")))?;
        let mut files = BTreeMap::new();
        for rel in &manifest.workflows {
            check_rel_path(rel)?;
            let p = dir.join(rel);
            let content = std::fs::read_to_string(&p).map_err(io(p.display().to_string()))?;
            files.insert(rel.clone(), content);
        }
        let mut pkg = Package {
            format: FORMAT,
            manifest,
            files,
            signature: SignatureBlock {
                publisher: publisher.name.clone(),
                public_key: publisher.public().public,
                signature: String::new(),
            },
        };
        pkg.check_contents()?;
        let sig = publisher.key.sign(&pkg.signed_bytes()?);
        pkg.signature.signature = hex(&sig.to_bytes());
        Ok(pkg)
    }

    /// Validates manifest and workflows (does not check the signature).
    fn check_contents(&self) -> Result<Vec<Workflow>, PkgError> {
        if self.format != FORMAT {
            return Err(PkgError::Invalid(format!(
                "unsupported package format {}",
                self.format
            )));
        }
        let m = &self.manifest;
        check_name(&m.name)?;
        check_version(&m.version)?;
        if m.workflows.is_empty() || m.workflows.len() > MAX_WORKFLOWS {
            return Err(PkgError::Invalid(format!(
                "a package holds 1-{MAX_WORKFLOWS} workflows"
            )));
        }
        let listed: Vec<&String> = m.workflows.iter().collect();
        let present: Vec<&String> = self.files.keys().collect();
        let mut sorted = listed.clone();
        sorted.sort();
        sorted.dedup();
        if sorted != present || sorted.len() != listed.len() {
            return Err(PkgError::Invalid(
                "files do not match the manifest's workflow list".into(),
            ));
        }
        let size: usize = self.files.values().map(String::len).sum();
        if size > MAX_PACKAGE_BYTES {
            return Err(PkgError::Invalid(format!(
                "package exceeds {MAX_PACKAGE_BYTES} bytes"
            )));
        }
        let mut ids = std::collections::HashSet::new();
        let mut out = Vec::new();
        for rel in &m.workflows {
            let wf = Workflow::from_toml(&self.files[rel])
                .map_err(|e| PkgError::Invalid(format!("{rel}: {e}")))?;
            wf.validate(&ValidationOptions::default()).map_err(|errs| {
                let list: Vec<String> = errs.iter().map(ToString::to_string).collect();
                PkgError::Invalid(format!("{rel}: {}", list.join("; ")))
            })?;
            if !ids.insert(wf.id.clone()) {
                return Err(PkgError::Invalid(format!(
                    "duplicate workflow id `{}`",
                    wf.id
                )));
            }
            out.push(wf);
        }
        Ok(out)
    }

    /// Serializes to the `.ambarpkg` file format.
    pub fn to_bytes(&self) -> Result<Vec<u8>, PkgError> {
        serde_json::to_vec_pretty(self).map_err(|e| PkgError::Invalid(e.to_string()))
    }

    /// Parses and fully verifies a `.ambarpkg` file: format, signature over
    /// the exact contents, manifest consistency and every workflow.
    pub fn verify(bytes: &[u8]) -> Result<Verified, PkgError> {
        if bytes.len() > MAX_PACKAGE_BYTES * 2 {
            return Err(PkgError::Invalid("file too large".into()));
        }
        let pkg: Package =
            serde_json::from_slice(bytes).map_err(|e| PkgError::Invalid(e.to_string()))?;
        let key = VerifyingKey::from_bytes(&unhex::<32>(&pkg.signature.public_key)?)
            .map_err(|e| PkgError::Verification(format!("bad public key: {e}")))?;
        let sig = Signature::from_bytes(&unhex::<64>(&pkg.signature.signature)?);
        key.verify_strict(&pkg.signed_bytes()?, &sig)
            .map_err(|_| PkgError::Verification("signature does not match the contents".into()))?;
        let workflows = pkg.check_contents()?;
        Ok(Verified {
            fingerprint: fingerprint(&pkg.signature.public_key),
            sha256: sha256_hex(bytes),
            package: pkg,
            workflows,
        })
    }
}

impl Verified {
    /// Risk review of every workflow, as `(workflow id, finding)`.
    pub fn risks(&self) -> Vec<(String, risk::Risk)> {
        self.workflows
            .iter()
            .flat_map(|w| risk::review(w).into_iter().map(move |r| (w.id.clone(), r)))
            .collect()
    }
}

// ------------------------------------------------------------------ trust

/// Directory of trusted publisher keys (`<fingerprint>.pub`).
#[derive(Debug, Clone)]
pub struct TrustStore {
    dir: PathBuf,
}

impl TrustStore {
    /// Opens (creating) a trust store directory.
    pub fn open(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Whether this key is trusted.
    pub fn is_trusted(&self, public_key_hex: &str) -> bool {
        let fp = fingerprint(public_key_hex);
        std::fs::read_to_string(self.dir.join(format!("{fp}.pub")))
            .ok()
            .and_then(|t| serde_json::from_str::<PublicKeyFile>(&t).ok())
            .is_some_and(|k| k.public.trim() == public_key_hex.trim())
    }

    /// Adds a key.
    pub fn trust(&self, key: &PublicKeyFile) -> Result<String, PkgError> {
        unhex::<32>(&key.public)?;
        std::fs::create_dir_all(&self.dir).map_err(io(self.dir.display().to_string()))?;
        let fp = key.fingerprint();
        let text =
            serde_json::to_string_pretty(key).map_err(|e| PkgError::Invalid(e.to_string()))?;
        std::fs::write(self.dir.join(format!("{fp}.pub")), text)
            .map_err(io(self.dir.display().to_string()))?;
        Ok(fp)
    }

    /// Removes a key by fingerprint.
    pub fn untrust(&self, fp: &str) -> bool {
        std::fs::remove_file(self.dir.join(format!("{fp}.pub"))).is_ok()
    }

    /// All trusted keys.
    pub fn list(&self) -> Vec<PublicKeyFile> {
        let mut out: Vec<PublicKeyFile> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| std::fs::read_to_string(e.path()).ok())
            .filter_map(|t| serde_json::from_str(&t).ok())
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// Fails with [`PkgError::Untrusted`] unless the package signer is trusted.
    pub fn require(&self, v: &Verified) -> Result<(), PkgError> {
        if self.is_trusted(&v.package.signature.public_key) {
            Ok(())
        } else {
            Err(PkgError::Untrusted {
                name: v.package.signature.publisher.clone(),
                fingerprint: v.fingerprint.clone(),
            })
        }
    }
}

// ------------------------------------------------------------------ marketplace

/// A marketplace index (`index.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    /// Index format version.
    pub format: u32,
    /// Marketplace name.
    pub name: String,
    /// Packages offered.
    pub packages: Vec<IndexEntry>,
}

/// One package in an index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    /// Package name.
    pub name: String,
    /// Version.
    pub version: String,
    /// Description.
    pub description: String,
    /// Author.
    pub author: String,
    /// Package location, relative to the index or absolute (`https://…` or a path).
    pub url: String,
    /// SHA-256 of the `.ambarpkg` file.
    pub sha256: String,
    /// Fingerprint of the signing key.
    pub signer: String,
}

impl Index {
    /// Parses an index.
    pub fn parse(bytes: &[u8]) -> Result<Self, PkgError> {
        let idx: Index =
            serde_json::from_slice(bytes).map_err(|e| PkgError::Invalid(format!("index: {e}")))?;
        if idx.format != 1 {
            return Err(PkgError::Invalid(format!(
                "unsupported index format {}",
                idx.format
            )));
        }
        Ok(idx)
    }

    /// Case-insensitive search over name, description and author.
    pub fn search(&self, term: &str) -> Vec<&IndexEntry> {
        let t = term.to_lowercase();
        self.packages
            .iter()
            .filter(|p| {
                t.is_empty()
                    || p.name.to_lowercase().contains(&t)
                    || p.description.to_lowercase().contains(&t)
                    || p.author.to_lowercase().contains(&t)
            })
            .collect()
    }

    /// Latest entry for a package name.
    pub fn find(&self, name: &str) -> Option<&IndexEntry> {
        self.packages
            .iter()
            .filter(|p| p.name == name)
            .max_by(|a, b| {
                let v =
                    |s: &str| -> Vec<u64> { s.split('.').filter_map(|x| x.parse().ok()).collect() };
                v(&a.version).cmp(&v(&b.version))
            })
    }

    /// Builds an index entry for a verified package stored at `url`.
    pub fn entry_for(v: &Verified, url: &str) -> IndexEntry {
        let m = &v.package.manifest;
        IndexEntry {
            name: m.name.clone(),
            version: m.version.clone(),
            description: m.description.clone(),
            author: m.author.clone(),
            url: url.into(),
            sha256: v.sha256.clone(),
            signer: v.fingerprint.clone(),
        }
    }
}

impl IndexEntry {
    /// Checks a downloaded file against this entry (hash, then signer).
    pub fn check(&self, bytes: &[u8]) -> Result<Verified, PkgError> {
        let got = sha256_hex(bytes);
        if got != self.sha256 {
            return Err(PkgError::Verification(format!(
                "download hash {got} does not match the index ({})",
                self.sha256
            )));
        }
        let v = Package::verify(bytes)?;
        if v.fingerprint != self.signer {
            return Err(PkgError::Verification(format!(
                "package is signed by {} but the index says {}",
                v.fingerprint, self.signer
            )));
        }
        if v.package.manifest.name != self.name || v.package.manifest.version != self.version {
            return Err(PkgError::Verification(
                "package name/version differ from the index".into(),
            ));
        }
        Ok(v)
    }
}

// ------------------------------------------------------------------ installed

/// Record of an installed package (`<data>/packages/<name>.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installed {
    /// Package name.
    pub name: String,
    /// Version.
    pub version: String,
    /// Signer fingerprint.
    pub signer: String,
    /// Publisher name.
    pub publisher: String,
    /// Workflow ids it added.
    pub workflows: Vec<String>,
    /// Install time (RFC 3339).
    pub installed_at: String,
}

/// Installed-package registry.
#[derive(Debug, Clone)]
pub struct Registry {
    dir: PathBuf,
}

impl Registry {
    /// Opens the registry in `<data_dir>/packages`.
    pub fn open(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join("packages"),
        }
    }

    /// Records an install.
    pub fn record(&self, i: &Installed) -> Result<(), PkgError> {
        std::fs::create_dir_all(&self.dir).map_err(io(self.dir.display().to_string()))?;
        let p = self.dir.join(format!("{}.json", i.name));
        let text = serde_json::to_string_pretty(i).map_err(|e| PkgError::Invalid(e.to_string()))?;
        std::fs::write(&p, text).map_err(io(p.display().to_string()))
    }

    /// Looks up an installed package.
    pub fn get(&self, name: &str) -> Option<Installed> {
        check_name(name).ok()?;
        let t = std::fs::read_to_string(self.dir.join(format!("{name}.json"))).ok()?;
        serde_json::from_str(&t).ok()
    }

    /// All installed packages.
    pub fn list(&self) -> Vec<Installed> {
        let mut out: Vec<Installed> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| std::fs::read_to_string(e.path()).ok())
            .filter_map(|t| serde_json::from_str(&t).ok())
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// Forgets a package.
    pub fn forget(&self, name: &str) -> bool {
        check_name(name).is_ok()
            && std::fs::remove_file(self.dir.join(format!("{name}.json"))).is_ok()
    }
}
