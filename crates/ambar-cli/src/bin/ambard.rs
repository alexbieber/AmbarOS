//! `ambard` — the Ambar OS background automation service.
//!
//! Normally started by `ambar service start`, launchd or systemd. Runs in the
//! foreground, logs to stderr, and shuts down gracefully on SIGTERM/SIGINT or
//! an IPC `shutdown` request.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use ambar_automation::{DenyAll, DesktopNotifier, LogNotifier, Notifier};
use ambar_service::{DaemonConfig, DaemonError, PathError, ServicePaths, start};
use clap::{Parser, ValueEnum};
use tokio::signal::unix::{SignalKind, signal};

#[derive(Parser, Debug)]
#[command(
    name = "ambard",
    version,
    about = "Ambar OS background automation service"
)]
struct Args {
    /// Data directory. Default: $XDG_DATA_HOME/ambar or ~/.local/share/ambar.
    #[arg(long, env = "AMBAR_DATA_DIR")]
    data_dir: Option<PathBuf>,
    /// Where notifications go.
    #[arg(long, value_enum, env = "AMBARD_NOTIFY", default_value_t = NotifyTarget::Desktop)]
    notify: NotifyTarget,
    /// Seconds to wait for running workflows on shutdown before cancelling them.
    #[arg(long, default_value_t = 30)]
    grace_secs: u64,
    /// Seconds a sensitive action waits for your approval (Automation Center or
    /// `ambar approval`). No answer means deny. 0 refuses them outright unless
    /// the workflow was added with --allow-sensitive.
    #[arg(long, env = "AMBARD_APPROVAL_TIMEOUT", default_value_t = 300)]
    approval_timeout: u64,
    /// Shortest schedule interval accepted (humantime, e.g. `10s`).
    #[arg(long, env = "AMBARD_MIN_INTERVAL", default_value = "10s", hide = true)]
    min_interval: humantime::Duration,
    /// Emit logs as JSON.
    #[arg(long)]
    json_logs: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum NotifyTarget {
    /// Native desktop notifications.
    Desktop,
    /// Log only.
    Log,
}

/// Exit code when another instance already owns the data directory.
const EXIT_ALREADY_RUNNING: u8 = 2;

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    let filter = tracing_subscriber::EnvFilter::try_from_env("AMBAR_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr);
    if args.json_logs {
        subscriber.json().init();
    } else {
        subscriber.init();
    }

    let Some(data_dir) = args.data_dir.or_else(ServicePaths::default_data_dir) else {
        eprintln!("ambard: HOME is not set; pass --data-dir");
        return ExitCode::FAILURE;
    };
    let notifier: Arc<dyn Notifier> = match args.notify {
        NotifyTarget::Desktop => Arc::new(DesktopNotifier),
        NotifyTarget::Log => Arc::new(LogNotifier),
    };
    // Sensitive actions that are not pre-approved wait for a live decision
    // (see --approval-timeout); with live approvals off they are refused.
    let mut config = DaemonConfig::new(ServicePaths::new(data_dir), notifier, Arc::new(DenyAll));
    config.grace = Duration::from_secs(args.grace_secs);
    if args.approval_timeout > 0 {
        config.live_approvals = Some(Duration::from_secs(args.approval_timeout));
    }
    config.engine.validation.min_interval = args.min_interval.into();

    let daemon = match start(config).await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("ambard: {e}");
            return match e {
                DaemonError::Path(PathError::AlreadyRunning { .. }) => {
                    ExitCode::from(EXIT_ALREADY_RUNNING)
                }
                _ => ExitCode::FAILURE,
            };
        }
    };

    let token = daemon.shutdown_token();
    match (
        signal(SignalKind::terminate()),
        signal(SignalKind::interrupt()),
    ) {
        (Ok(mut term), Ok(mut int)) => {
            tokio::spawn(async move {
                tokio::select! {
                    _ = term.recv() => tracing::info!("SIGTERM received"),
                    _ = int.recv() => tracing::info!("SIGINT received"),
                    _ = token.cancelled() => {}
                }
                token.cancel();
            });
        }
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("ambard: could not install signal handlers: {e}");
            daemon.stop().await;
            return ExitCode::FAILURE;
        }
    }

    let report = daemon.wait().await;
    if report.interrupted > 0 {
        tracing::warn!(
            interrupted = report.interrupted,
            "some runs were cancelled at shutdown"
        );
    }
    ExitCode::SUCCESS
}
