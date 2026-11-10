//! `ambar service …` and `ambar workflow …`: talk to the ambard background service.

use std::fs::OpenOptions;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use ambar_automation::ValidationOptions;
use ambar_service::protocol::{GrantSpec, Health, WorkflowInfo};
use ambar_service::units::{LAUNCHD_LABEL, Manager, SYSTEMD_UNIT};
use ambar_service::{Client, ClientError, ServicePaths};
use anyhow::{Context, Result, bail};
use clap::Subcommand;

/// Exit code for "service not running", as with `systemctl status`.
pub const EXIT_NOT_RUNNING: i32 = 3;

const CLIENT_NAME: &str = "ambar-cli";

#[derive(Subcommand, Debug)]
pub enum ServiceCmd {
    /// Start ambard in the background (through launchd/systemd if installed).
    Start,
    /// Stop ambard gracefully. Running workflows finish their current run.
    Stop,
    /// Show whether ambard is running and healthy. Exit 0 ok, 1 degraded, 3 not running.
    Status {
        /// Print the raw health report as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Install a launchd agent (macOS) or systemd user unit (Linux) so ambard
    /// starts at login and enabled workflows survive a reboot.
    Install,
    /// Remove the launchd agent / systemd user unit.
    Uninstall,
}

#[derive(Subcommand, Debug)]
pub enum WorkflowCmd {
    /// Add a workflow file to the service, after you approve its permissions.
    Add {
        /// Workflow TOML file.
        file: PathBuf,
        /// Approve the requested permissions without asking.
        #[arg(long)]
        yes: bool,
        /// Pre-approve sensitive actions (trash, overwrite). In the background
        /// nobody can be asked, so without this they are refused.
        #[arg(long)]
        allow_sensitive: bool,
        /// Enable it right away.
        #[arg(long)]
        enable: bool,
        /// Replace an existing workflow with the same id.
        #[arg(long)]
        replace: bool,
    },
    /// List workflows in the service.
    List {
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Start a workflow's trigger. It stays enabled across restarts.
    Enable {
        /// Workflow id.
        id: String,
    },
    /// Stop a workflow's trigger. A run in progress finishes.
    Disable {
        /// Workflow id.
        id: String,
    },
    /// Skip new firings and hold a run in progress before its next step.
    Pause {
        /// Workflow id.
        id: String,
    },
    /// Resume a paused workflow.
    Resume {
        /// Workflow id.
        id: String,
    },
    /// Stop the trigger and abort a run in progress at its next step.
    Cancel {
        /// Workflow id.
        id: String,
    },
    /// Run a manual or scheduled workflow now and wait for the result.
    Run {
        /// Workflow id.
        id: String,
    },
}

async fn connect(paths: &ServicePaths) -> Result<Client> {
    match Client::connect(paths, CLIENT_NAME).await {
        Ok(c) => Ok(c),
        Err(ClientError::NotRunning(_)) => {
            bail!("ambard is not running. Start it with `ambar service start`.")
        }
        Err(e) => Err(e.into()),
    }
}

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")
}

/// Locates the ambard binary: `$AMBARD_BIN`, else next to this executable.
fn ambard_bin() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("AMBARD_BIN") {
        return Ok(PathBuf::from(p));
    }
    let exe = std::env::current_exe().context("locating the ambar executable")?;
    let candidate = exe
        .parent()
        .map(|d| d.join("ambard"))
        .context("ambar executable has no parent directory")?;
    if candidate.is_file() {
        Ok(candidate)
    } else {
        bail!(
            "could not find ambard next to {}; set AMBARD_BIN",
            exe.display()
        )
    }
}

fn installed_unit() -> Option<(Manager, PathBuf)> {
    let manager = Manager::current();
    let path = manager.unit_path(&home().ok()?);
    path.is_file().then_some((manager, path))
}

fn run_tool(program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("running {program}"))?;
    if !status.success() {
        bail!("`{program} {}` failed ({status})", args.join(" "));
    }
    Ok(())
}

fn launchd_target() -> String {
    format!(
        "gui/{}/{LAUNCHD_LABEL}",
        ambar_service::paths::current_uid()
    )
}

async fn wait_for_service(paths: &ServicePaths, timeout: Duration) -> Option<Health> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(mut c) = Client::connect(paths, CLIENT_NAME).await
            && let Ok(h) = c.health().await
        {
            return Some(h);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    None
}

async fn wait_for_exit(paths: &ServicePaths, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !paths.is_locked() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

fn print_health(h: &Health) {
    println!("ambard is running ({})", h.status);
    println!("  pid        {}", h.pid);
    println!("  version    {}", h.version);
    println!(
        "  uptime     {}",
        humantime::format_duration(Duration::from_secs(h.uptime_secs))
    );
    println!("  data dir   {}", h.data_dir);
    println!(
        "  database   {}",
        if h.database_ok { "ok" } else { "ERROR" }
    );
    let w = &h.workflows;
    println!(
        "  workflows  {} active, {} paused, {} disabled, {} cancelled ({} running now)",
        w.active, w.paused, w.inactive, w.cancelled, h.running
    );
    if h.recovered_interrupted_runs > 0 {
        println!(
            "  recovered  {} run(s) interrupted by an earlier crash",
            h.recovered_interrupted_runs
        );
    }
    for p in &h.problems {
        println!("  problem    {p}");
    }
}

pub async fn service(cmd: &ServiceCmd, paths: &ServicePaths) -> Result<i32> {
    match cmd {
        ServiceCmd::Status { json } => match Client::connect(paths, CLIENT_NAME).await {
            Ok(mut c) => {
                let h = c.health().await?;
                if *json {
                    println!("{}", serde_json::to_string_pretty(&h)?);
                } else {
                    print_health(&h);
                }
                Ok(if h.status == "ok" { 0 } else { 1 })
            }
            Err(ClientError::NotRunning(_)) if paths.is_locked() => {
                println!("ambard holds the lock but is not answering (starting or stuck)");
                Ok(1)
            }
            Err(ClientError::NotRunning(_)) => {
                println!("ambard is not running");
                Ok(EXIT_NOT_RUNNING)
            }
            Err(e) => Err(e.into()),
        },

        ServiceCmd::Start => {
            if let Ok(mut c) = Client::connect(paths, CLIENT_NAME).await {
                let h = c.health().await?;
                println!("ambard is already running (pid {})", h.pid);
                return Ok(0);
            }
            if paths.is_locked() {
                bail!("ambard holds the lock but is not answering; check its log");
            }
            if let Some((manager, _)) = installed_unit() {
                match manager {
                    Manager::Launchd => run_tool("launchctl", &["kickstart", &launchd_target()])?,
                    Manager::Systemd => run_tool("systemctl", &["--user", "start", SYSTEMD_UNIT])?,
                }
            } else {
                spawn_detached(paths)?;
            }
            match wait_for_service(paths, Duration::from_secs(10)).await {
                Some(h) => {
                    println!("ambard started (pid {})", h.pid);
                    if h.status != "ok" {
                        print_health(&h);
                    }
                    Ok(0)
                }
                None => bail!(
                    "ambard did not come up within 10s; see {}",
                    paths.log_dir().join("ambard.log").display()
                ),
            }
        }

        ServiceCmd::Stop => {
            let mut c = match Client::connect(paths, CLIENT_NAME).await {
                Ok(c) => c,
                Err(ClientError::NotRunning(_)) if !paths.is_locked() => {
                    println!("ambard is not running");
                    return Ok(0);
                }
                Err(e) => return Err(e.into()),
            };
            match installed_unit() {
                // Let systemd own the stop so its view of the unit stays right.
                Some((Manager::Systemd, _)) => {
                    drop(c);
                    run_tool("systemctl", &["--user", "stop", SYSTEMD_UNIT])?;
                }
                // launchd: a clean exit (0) is not restarted (KeepAlive SuccessfulExit=false).
                _ => c.shutdown().await?,
            }
            if wait_for_exit(paths, Duration::from_secs(60)).await {
                println!("ambard stopped");
                Ok(0)
            } else {
                bail!("ambard did not stop within 60s")
            }
        }

        ServiceCmd::Install => {
            let manager = Manager::current();
            let bin = ambard_bin()?
                .canonicalize()
                .context("resolving ambard path")?;
            let unit = manager.unit_path(&home()?);
            if Client::connect(paths, CLIENT_NAME).await.is_ok() {
                println!("stopping the running ambard so the service manager can take over…");
                Box::pin(service(&ServiceCmd::Stop, paths)).await?;
            }
            paths.ensure()?;
            std::fs::create_dir_all(paths.log_dir())?;
            if let Some(parent) = unit.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(
                &unit,
                manager.render(&bin, &paths.data_dir, &paths.log_dir()),
            )
            .with_context(|| format!("writing {}", unit.display()))?;
            println!("wrote {}", unit.display());
            match manager {
                Manager::Launchd => {
                    let _ = Command::new("launchctl")
                        .args(["bootout", &launchd_target()])
                        .stderr(Stdio::null())
                        .status();
                    let domain = format!("gui/{}", ambar_service::paths::current_uid());
                    run_tool(
                        "launchctl",
                        &["bootstrap", &domain, &unit.display().to_string()],
                    )?;
                }
                Manager::Systemd => {
                    run_tool("systemctl", &["--user", "daemon-reload"])?;
                    run_tool("systemctl", &["--user", "enable", "--now", SYSTEMD_UNIT])?;
                }
            }
            match wait_for_service(paths, Duration::from_secs(10)).await {
                Some(h) => println!(
                    "ambard installed and running (pid {}); it will start at login",
                    h.pid
                ),
                None => println!(
                    "installed, but ambard is not answering yet; check `ambar service status`"
                ),
            }
            Ok(0)
        }

        ServiceCmd::Uninstall => {
            let Some((manager, unit)) = installed_unit() else {
                println!("no ambard service unit is installed");
                return Ok(0);
            };
            match manager {
                Manager::Launchd => run_tool("launchctl", &["bootout", &launchd_target()])?,
                Manager::Systemd => {
                    run_tool("systemctl", &["--user", "disable", "--now", SYSTEMD_UNIT])?;
                }
            }
            std::fs::remove_file(&unit).with_context(|| format!("removing {}", unit.display()))?;
            if manager == Manager::Systemd {
                let _ = run_tool("systemctl", &["--user", "daemon-reload"]);
            }
            println!("removed {}", unit.display());
            Ok(0)
        }
    }
}

/// Starts ambard without a service manager. The process gets its own process
/// group so Ctrl-C in this terminal does not reach it, and logs to a file.
fn spawn_detached(paths: &ServicePaths) -> Result<()> {
    let bin = ambard_bin()?;
    paths.ensure()?;
    std::fs::create_dir_all(paths.log_dir())?;
    let log_path = paths.log_dir().join("ambard.log");
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("opening {}", log_path.display()))?;
    let mut child = Command::new(&bin)
        .arg("--data-dir")
        .arg(&paths.data_dir)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .process_group(0)
        .spawn()
        .with_context(|| format!("starting {}", bin.display()))?;
    // Catch immediate failures (bad flags, lock held) with a clear message.
    std::thread::sleep(Duration::from_millis(150));
    if let Some(status) = child.try_wait()? {
        let tail = std::fs::read_to_string(&log_path).unwrap_or_default();
        let last: Vec<&str> = tail.lines().rev().take(5).collect();
        bail!(
            "ambard exited immediately ({status}):\n{}",
            last.into_iter().rev().collect::<Vec<_>>().join("\n")
        );
    }
    Ok(())
}

fn print_list(list: &[WorkflowInfo]) {
    if list.is_empty() {
        println!("No workflows. Add one with `ambar workflow add <file>`.");
        return;
    }
    println!(
        "{:<24} {:<10} {:<8} {:<13} LAST RUN",
        "ID", "STATE", "RUNNING", "TRIGGER"
    );
    for w in list {
        let state = if w.state == "inactive" {
            "disabled"
        } else {
            &w.state
        };
        let last = match &w.last_run {
            Some(r) => {
                let when = chrono::DateTime::parse_from_rfc3339(&r.started_at)
                    .map(|t| {
                        t.with_timezone(&chrono::Local)
                            .format("%Y-%m-%d %H:%M:%S")
                            .to_string()
                    })
                    .unwrap_or_else(|_| r.started_at.clone());
                format!("{} {when}", r.status)
            }
            None => "-".into(),
        };
        let grant = if w.granted {
            ""
        } else {
            " (no permission grant)"
        };
        println!(
            "{:<24} {:<10} {:<8} {:<13} {last}{grant}",
            w.id,
            state,
            if w.running { "yes" } else { "no" },
            w.trigger
        );
    }
}

pub async fn workflow(cmd: &WorkflowCmd, paths: &ServicePaths) -> Result<i32> {
    let mut c = connect(paths).await?;
    match cmd {
        WorkflowCmd::Add {
            file,
            yes,
            allow_sensitive,
            enable,
            replace,
        } => {
            // Check locally for a friendly error before asking for consent.
            // The service validates again with its own limits.
            let opts = ValidationOptions {
                min_interval: Duration::ZERO,
            };
            let wf = crate::load_with(file, &opts)?;
            crate::describe(&wf);
            if *allow_sensitive {
                println!("  Sensitive actions (trash, overwrite) will run without asking.");
            }
            println!();
            if !*yes && !crate::ask("Grant these permissions to run in the background? [y/N] ") {
                bail!("permissions not granted; the workflow was not added");
            }
            let source = std::fs::read_to_string(file)?;
            let grant = Some(GrantSpec {
                allow_sensitive: *allow_sensitive,
            });
            let info = c.register(&source, grant, *replace).await?;
            if *enable {
                c.enable(&info.id).await?;
                println!("added and enabled `{}`", info.id);
            } else {
                println!(
                    "added `{}` (disabled). Enable it with `ambar workflow enable {}`",
                    info.id, info.id
                );
            }
        }
        WorkflowCmd::List { json } => {
            let list = c.list().await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&list)?);
            } else {
                print_list(&list);
            }
        }
        WorkflowCmd::Enable { id } => {
            c.enable(id).await?;
            println!("enabled `{id}`");
        }
        WorkflowCmd::Disable { id } => {
            c.disable(id).await?;
            println!("disabled `{id}`");
        }
        WorkflowCmd::Pause { id } => {
            c.pause(id).await?;
            println!("paused `{id}`");
        }
        WorkflowCmd::Resume { id } => {
            c.resume(id).await?;
            println!("resumed `{id}`");
        }
        WorkflowCmd::Cancel { id } => {
            c.cancel(id).await?;
            println!("cancelled `{id}`");
        }
        WorkflowCmd::Run { id } => {
            let r = c.run_now(id).await?;
            crate::print_run(&r, true);
            if r.status != ambar_automation::ExecutionStatus::Succeeded {
                return Ok(1);
            }
        }
    }
    Ok(0)
}
