//! Service-manager integration: launchd (macOS) and systemd (Linux) user units.
//!
//! Both run `ambard` as the logged-in user (never root), start it at login so
//! enabled workflows are restored after a reboot, and restart it only if it
//! crashes. A graceful `ambar service stop` exits 0 and stays stopped.

use std::path::{Path, PathBuf};

/// launchd label.
pub const LAUNCHD_LABEL: &str = "os.ambar.ambard";
/// systemd unit name.
pub const SYSTEMD_UNIT: &str = "ambard.service";

/// Which service manager this platform uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Manager {
    /// macOS launchd (per-user LaunchAgent).
    Launchd,
    /// systemd user manager.
    Systemd,
}

impl Manager {
    /// The manager for the current platform.
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Manager::Launchd
        } else {
            Manager::Systemd
        }
    }

    /// Where the unit file goes, under `home`.
    pub fn unit_path(&self, home: &Path) -> PathBuf {
        match self {
            Manager::Launchd => home
                .join("Library/LaunchAgents")
                .join(format!("{LAUNCHD_LABEL}.plist")),
            Manager::Systemd => home.join(".config/systemd/user").join(SYSTEMD_UNIT),
        }
    }

    /// Renders the unit file.
    pub fn render(&self, ambard: &Path, data_dir: &Path, log_dir: &Path) -> String {
        match self {
            Manager::Launchd => launchd_plist(ambard, data_dir, log_dir),
            Manager::Systemd => systemd_unit(ambard, data_dir),
        }
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// launchd LaunchAgent plist.
pub fn launchd_plist(ambard: &Path, data_dir: &Path, log_dir: &Path) -> String {
    let e = |p: &Path| xml_escape(&p.display().to_string());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{bin}</string>
        <string>--data-dir</string>
        <string>{data}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
    <key>ThrottleInterval</key>
    <integer>10</integer>
    <key>ProcessType</key>
    <string>Background</string>
    <key>StandardOutPath</key>
    <string>{log}/ambard.log</string>
    <key>StandardErrorPath</key>
    <string>{log}/ambard.log</string>
</dict>
</plist>
"#,
        label = LAUNCHD_LABEL,
        bin = e(ambard),
        data = e(data_dir),
        log = e(log_dir),
    )
}

/// Quotes a value for a systemd `ExecStart=` line.
fn systemd_quote(s: &str) -> String {
    let escaped = s
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%")
        .replace('$', "$$");
    format!("\"{escaped}\"")
}

/// systemd user unit.
pub fn systemd_unit(ambard: &Path, data_dir: &Path) -> String {
    format!(
        "[Unit]
Description=Ambar automation service
Documentation=https://github.com/alexbieber/AmbarOS
After=default.target

[Service]
Type=simple
ExecStart={bin} --data-dir {data}
Restart=on-failure
RestartSec=5
TimeoutStopSec=45
KillSignal=SIGTERM
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ReadWritePaths=%h
UMask=0077

[Install]
WantedBy=default.target
",
        bin = systemd_quote(&ambard.display().to_string()),
        data = systemd_quote(&data_dir.display().to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_is_well_formed_and_escaped() {
        let p = launchd_plist(
            Path::new("/Applications/Ambar & Co/ambard"),
            Path::new("/Users/a/.local/share/ambar"),
            Path::new("/Users/a/.local/share/ambar/logs"),
        );
        assert!(p.contains("<string>/Applications/Ambar &amp; Co/ambard</string>"));
        assert!(p.contains(LAUNCHD_LABEL));
        assert!(p.contains("<key>RunAtLoad</key>\n    <true/>"));
        assert!(p.contains("<key>SuccessfulExit</key>\n        <false/>"));
        // Balanced dict/array tags.
        assert_eq!(p.matches("<dict>").count(), p.matches("</dict>").count());
        assert_eq!(p.matches("<array>").count(), p.matches("</array>").count());
    }

    #[test]
    fn systemd_unit_quotes_paths() {
        let u = systemd_unit(
            Path::new("/opt/ambar bin/ambard"),
            Path::new("/home/a/100%"),
        );
        assert!(
            u.contains(r#"ExecStart="/opt/ambar bin/ambard" --data-dir "/home/a/100%%""#),
            "{u}"
        );
        assert!(u.contains("Restart=on-failure"));
        assert!(u.contains("WantedBy=default.target"));
        assert!(u.contains("NoNewPrivileges=true"));
    }

    #[test]
    fn unit_paths() {
        let h = Path::new("/home/a");
        assert_eq!(
            Manager::Systemd.unit_path(h),
            Path::new("/home/a/.config/systemd/user/ambard.service")
        );
        assert_eq!(
            Manager::Launchd.unit_path(h),
            Path::new("/home/a/Library/LaunchAgents/os.ambar.ambard.plist")
        );
    }
}

#[cfg(test)]
mod external_validation {
    use super::*;

    /// With `AMBAR_DUMP_UNITS=<dir>`, writes both unit files there so CI can
    /// run `systemd-analyze verify` and `plutil -lint` / `xmllint` on them.
    #[test]
    fn dump_units_for_external_validators() -> std::io::Result<()> {
        let Some(dir) = std::env::var_os("AMBAR_DUMP_UNITS") else {
            return Ok(());
        };
        let dir = Path::new(&dir);
        let bin = dir.join("ambard");
        let data = dir.join("data");
        std::fs::write(dir.join(SYSTEMD_UNIT), systemd_unit(&bin, &data))?;
        std::fs::write(
            dir.join(format!("{LAUNCHD_LABEL}.plist")),
            launchd_plist(&bin, &data, &data.join("logs")),
        )
    }
}
