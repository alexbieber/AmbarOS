//! On-disk layout of the service and the single-instance lock.
//!
//! ```text
//! <data_dir>/            0700  (default: $XDG_DATA_HOME/ambar or ~/.local/share/ambar)
//!   ambar.db                   workflows, grants, history, audit (SQLite)
//!   trash/                     recoverable trash
//!   logs/ambard.log            daemon log when started without a service manager
//!   run/                 0700  runtime files, owned by the service user
//!     ambard.lock              exclusive lock + pid: one engine per data dir
//!     ambard.sock        0600  IPC socket
//!     ambard.token       0600  per-start secret the CLI presents on connect
//! ```

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// `sockaddr_un.sun_path` is 104 bytes on macOS and 108 on Linux, including the NUL.
pub const MAX_SOCKET_PATH: usize = 103;

/// Errors preparing service paths.
#[derive(Debug, thiserror::Error)]
pub enum PathError {
    /// I/O failure.
    #[error("{context}: {source}")]
    Io {
        /// What was being done.
        context: String,
        /// Underlying error.
        source: io::Error,
    },
    /// A runtime directory is owned by another user.
    #[error("{0} is owned by another user; refusing to use it")]
    WrongOwner(PathBuf),
    /// Another engine holds the lock.
    #[error("ambard is already running for {data_dir} (pid {pid})")]
    AlreadyRunning {
        /// Data directory.
        data_dir: PathBuf,
        /// Pid recorded in the lock file, if readable.
        pid: String,
    },
    /// Socket path too long for the platform.
    #[error(
        "socket path {0} is too long ({1} bytes; max {MAX_SOCKET_PATH}); use a shorter --data-dir"
    )]
    SocketPathTooLong(PathBuf, usize),
}

fn io_err(context: impl Into<String>) -> impl FnOnce(io::Error) -> PathError {
    let context = context.into();
    move |source| PathError::Io { context, source }
}

/// The current effective user id.
pub fn current_uid() -> u32 {
    rustix::process::geteuid().as_raw()
}

/// Paths used by the service for one data directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServicePaths {
    /// Root data directory.
    pub data_dir: PathBuf,
}

impl ServicePaths {
    /// Paths rooted at `data_dir`.
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    /// `$XDG_DATA_HOME/ambar`, else `~/.local/share/ambar`.
    pub fn default_data_dir() -> Option<PathBuf> {
        if let Some(x) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
            return Some(PathBuf::from(x).join("ambar"));
        }
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share/ambar"))
    }

    /// SQLite database.
    pub fn db(&self) -> PathBuf {
        self.data_dir.join("ambar.db")
    }
    /// Runtime directory.
    pub fn run_dir(&self) -> PathBuf {
        self.data_dir.join("run")
    }
    /// IPC socket.
    pub fn socket(&self) -> PathBuf {
        self.run_dir().join("ambard.sock")
    }
    /// Auth token file.
    pub fn token(&self) -> PathBuf {
        self.run_dir().join("ambard.token")
    }
    /// Lock file.
    pub fn lock(&self) -> PathBuf {
        self.run_dir().join("ambard.lock")
    }
    /// Log directory.
    pub fn log_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    /// Creates the data and runtime directories with mode 0700 and checks
    /// that they belong to the current user.
    pub fn ensure(&self) -> Result<(), PathError> {
        for dir in [self.data_dir.clone(), self.run_dir()] {
            ensure_private_dir(&dir)?;
        }
        let sock = self.socket();
        let len = sock.as_os_str().len();
        if len > MAX_SOCKET_PATH {
            return Err(PathError::SocketPathTooLong(sock, len));
        }
        Ok(())
    }

    /// Takes the exclusive single-instance lock. The lock is released when
    /// the returned guard is dropped *or the process dies*, so a crashed
    /// daemon never blocks a restart.
    pub fn acquire_lock(&self) -> Result<InstanceLock, PathError> {
        self.ensure()?;
        let path = self.lock();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&path)
            .map_err(io_err(format!("opening {}", path.display())))?;
        match file.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => {
                let pid = fs::read_to_string(&path)
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default();
                return Err(PathError::AlreadyRunning {
                    data_dir: self.data_dir.clone(),
                    pid: if pid.is_empty() {
                        "unknown".into()
                    } else {
                        pid
                    },
                });
            }
            Err(fs::TryLockError::Error(e)) => {
                return Err(io_err(format!("locking {}", path.display()))(e));
            }
        }
        file.set_len(0)
            .and_then(|()| writeln!(file, "{}", std::process::id()))
            .map_err(io_err("writing pid to lock file"))?;
        Ok(InstanceLock { _file: file })
    }

    /// Whether some process currently holds the lock.
    pub fn is_locked(&self) -> bool {
        let Ok(file) = File::open(self.lock()) else {
            return false;
        };
        matches!(file.try_lock_shared(), Err(fs::TryLockError::WouldBlock))
    }
}

/// Held for as long as an engine owns the data directory.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

/// Creates `dir` (mode 0700) if missing, tightens its mode if it exists, and
/// refuses directories owned by someone else.
pub fn ensure_private_dir(dir: &Path) -> Result<(), PathError> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(io_err(format!("creating {}", dir.display())))?;
    let meta = fs::metadata(dir).map_err(io_err(format!("reading {}", dir.display())))?;
    if meta.uid() != current_uid() {
        return Err(PathError::WrongOwner(dir.to_path_buf()));
    }
    if meta.permissions().mode() & 0o077 != 0 {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
            .map_err(io_err(format!("restricting {}", dir.display())))?;
    }
    Ok(())
}

/// Writes `contents` to `path` with mode 0600, atomically (temp file + rename).
pub fn write_private_file(path: &Path, contents: &[u8]) -> Result<(), PathError> {
    let tmp = path.with_extension("tmp");
    let _ = fs::remove_file(&tmp);
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(io_err(format!("creating {}", tmp.display())))?;
    f.write_all(contents)
        .and_then(|()| f.sync_all())
        .map_err(io_err(format!("writing {}", tmp.display())))?;
    fs::rename(&tmp, path).map_err(io_err(format!("installing {}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirs_are_private_and_lock_is_exclusive() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let paths = ServicePaths::new(dir.path().join("d"));
        let lock = paths.acquire_lock()?;
        for d in [paths.data_dir.clone(), paths.run_dir()] {
            assert_eq!(fs::metadata(&d)?.permissions().mode() & 0o777, 0o700);
        }
        assert!(paths.is_locked());
        let second = paths.acquire_lock();
        assert!(
            matches!(second, Err(PathError::AlreadyRunning { .. })),
            "{second:?}"
        );
        drop(lock);
        assert!(!paths.is_locked());
        let _again = paths.acquire_lock()?;
        Ok(())
    }

    #[test]
    fn loose_permissions_are_tightened() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let d = dir.path().join("x");
        fs::create_dir(&d)?;
        fs::set_permissions(&d, fs::Permissions::from_mode(0o777))?;
        ensure_private_dir(&d)?;
        assert_eq!(fs::metadata(&d)?.permissions().mode() & 0o777, 0o700);
        Ok(())
    }

    #[test]
    fn private_files_are_0600() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let f = dir.path().join("t");
        write_private_file(&f, b"secret")?;
        write_private_file(&f, b"secret2")?;
        assert_eq!(fs::metadata(&f)?.permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::read(&f)?, b"secret2");
        Ok(())
    }

    #[test]
    fn long_socket_paths_are_rejected() {
        let paths = ServicePaths::new(format!("/tmp/{}", "x".repeat(120)));
        assert!(matches!(
            paths.ensure(),
            Err(PathError::SocketPathTooLong(..)) | Err(PathError::Io { .. })
        ));
    }
}
