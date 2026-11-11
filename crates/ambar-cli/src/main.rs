//! `ambar` — command-line front end for the Ambar automation engine.

mod agent_cmds;
mod ai_cmds;
mod pkg_cmds;
mod service_cmds;

use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ambar_automation::{
    ApprovalRequest, Approver, DesktopNotifier, Engine, EngineConfig, EngineEvent, ExecutionRecord,
    LogNotifier, Notifier, Store, Trigger, ValidationOptions, Workflow,
};
use ambar_service::{PathError, ServicePaths};
use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(name = "ambar", version, about = "Ambar OS automation engine")]
struct Cli {
    /// Data directory (database and trash). Default: $XDG_DATA_HOME/ambar.
    #[arg(long, global = true, env = "AMBAR_DATA_DIR")]
    data_dir: Option<PathBuf>,
    /// Emit logs as JSON.
    #[arg(long, global = true)]
    json_logs: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Check a workflow file and show what it will do and need.
    Validate {
        /// Workflow TOML file.
        file: PathBuf,
    },
    /// Register a workflow, ask for its permissions, and run it.
    ///
    /// Manual workflows run once. Scheduled and folder workflows keep running
    /// until Ctrl-C (or a single run with --once for schedules).
    Run {
        /// Workflow TOML file.
        file: PathBuf,
        /// Grant the requested permissions without asking.
        #[arg(long)]
        yes: bool,
        /// Pre-approve sensitive actions (trash, overwrite) for this workflow.
        #[arg(long)]
        allow_sensitive: bool,
        /// Run a scheduled workflow once immediately and exit.
        #[arg(long)]
        once: bool,
        /// Where notifications go.
        #[arg(long, value_enum, default_value_t = NotifyTarget::Desktop)]
        notify: NotifyTarget,
    },
    /// Show recent executions.
    History {
        /// Only this workflow.
        #[arg(long)]
        workflow: Option<String>,
        /// Number of runs to show.
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Include each step.
        #[arg(long)]
        steps: bool,
    },
    /// Show the security audit log.
    Audit {
        /// Only this workflow.
        #[arg(long)]
        workflow: Option<String>,
    },
    /// List stored workflows.
    Workflows,
    /// Withdraw a workflow's permission grant.
    Revoke {
        /// Workflow id.
        workflow: String,
    },
    /// Manage the ambard background service.
    #[command(subcommand)]
    Service(service_cmds::ServiceCmd),
    /// Manage workflows in the running ambard service.
    #[command(subcommand)]
    Workflow(service_cmds::WorkflowCmd),
    /// Build automations from plain-language descriptions.
    #[command(subcommand)]
    Ai(ai_cmds::AiCmd),
    /// Manage agent identities and review their proposals.
    #[command(subcommand)]
    Agent(agent_cmds::AgentCmd),
    /// Allow or deny sensitive actions that are waiting for you.
    #[command(subcommand)]
    Approval(agent_cmds::ApprovalCmd),
    /// Build, sign, share and install workflow packages.
    #[command(subcommand)]
    Pkg(pkg_cmds::PkgCmd),
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum NotifyTarget {
    /// Native desktop notification (notify-send / osascript).
    Desktop,
    /// Log only.
    Log,
}

/// Asks the person at the terminal before sensitive actions.
#[derive(Debug)]
struct TerminalApprover;

impl Approver for TerminalApprover {
    fn approve(&self, r: &ApprovalRequest) -> bool {
        if !io::stdin().is_terminal() {
            eprintln!("refused (no terminal to ask): {}", r.description);
            return false;
        }
        ask(&format!(
            "Workflow `{}` wants to {}. Allow? [y/N] ",
            r.workflow_id, r.description
        ))
    }
}

pub(crate) fn ask(prompt: &str) -> bool {
    eprint!("{prompt}");
    let _ = io::stderr().flush();
    let mut line = String::new();
    if io::stdin().lock().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
}

fn data_dir(cli: &Cli) -> Result<PathBuf> {
    if let Some(d) = &cli.data_dir {
        return Ok(d.clone());
    }
    ServicePaths::default_data_dir().context("HOME is not set; pass --data-dir")
}

fn open_store(dir: &Path) -> Result<Store> {
    Store::open(&dir.join("ambar.db")).context("opening the Ambar database")
}

fn load(file: &Path) -> Result<Workflow> {
    load_with(file, &ValidationOptions::default())
}

pub(crate) fn load_with(file: &Path, opts: &ValidationOptions) -> Result<Workflow> {
    let wf = Workflow::from_file(file).with_context(|| format!("reading {}", file.display()))?;
    if let Err(errors) = wf.validate(opts) {
        let list: Vec<String> = errors.iter().map(|e| format!("  - {e}")).collect();
        bail!("{} is invalid:\n{}", file.display(), list.join("\n"));
    }
    Ok(wf)
}

pub(crate) fn describe(wf: &Workflow) {
    println!("{} ({})", wf.name, wf.id);
    if !wf.description.is_empty() {
        println!("  {}", wf.description);
    }
    let trigger = match &wf.trigger {
        Trigger::Manual => "manual".to_string(),
        Trigger::Schedule { every: Some(e), .. } => format!("every {e}"),
        Trigger::Schedule { cron: Some(c), .. } => format!("cron `{c}` (local time)"),
        Trigger::Schedule { .. } => "schedule".to_string(),
        Trigger::FolderWatch {
            path,
            events,
            pattern,
            recursive,
        } => format!(
            "when files are {} in {}{}{}",
            events
                .iter()
                .map(|e| e.as_str())
                .collect::<Vec<_>>()
                .join("/"),
            path,
            pattern
                .as_ref()
                .map(|p| format!(" matching {p}"))
                .unwrap_or_default(),
            if *recursive {
                " (including subfolders)"
            } else {
                ""
            }
        ),
    };
    println!("  Trigger:  {trigger}");
    for (i, a) in wf.actions.iter().enumerate() {
        let flag = if a.is_always_sensitive() {
            "  [needs approval]"
        } else {
            ""
        };
        println!("  Step {}:   {}{flag}", i + 1, a.kind());
    }
    print_permissions(wf);
}

fn print_permissions(wf: &Workflow) {
    let p = &wf.permissions;
    println!("  Permissions requested:");
    for r in &p.read {
        println!("    read   {r}");
    }
    for w in &p.write {
        println!("    write  {w}");
    }
    if p.notify {
        println!("    send notifications");
    }
    if p.read.is_empty() && p.write.is_empty() && !p.notify {
        println!("    (none)");
    }
}

pub(crate) fn print_run(r: &ExecutionRecord, steps: bool) {
    let when = r
        .started_at
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H:%M:%S");
    let id: String = r.id.chars().take(8).collect();
    print!(
        "{when}  {:<10} {:<20} {id}  {}",
        r.status.as_str(),
        r.workflow_id,
        r.trigger
    );
    if let Some(e) = &r.error {
        print!("\n    {e}");
    }
    println!();
    if steps {
        for s in &r.steps {
            println!(
                "    {}. {:<16} {:<9} {}",
                s.index + 1,
                s.action,
                s.status.as_str(),
                s.message
            );
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let filter = tracing_subscriber::EnvFilter::try_from_env("AMBAR_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(io::stderr);
    if cli.json_logs {
        subscriber.json().init();
    } else {
        subscriber.init();
    }

    match &cli.command {
        Command::Validate { file } => {
            let wf = load(file)?;
            println!("✓ valid\n");
            describe(&wf);
        }
        Command::Run {
            file,
            yes,
            allow_sensitive,
            once,
            notify,
        } => {
            let wf = load(file)?;
            let dir = data_dir(&cli)?;
            // One engine per data directory: refuse while ambard owns it, so a
            // workflow can never run twice.
            let _lock = ServicePaths::new(&dir)
                .acquire_lock()
                .map_err(|e| match e {
                    PathError::AlreadyRunning { pid, .. } => anyhow!(
                        "ambard (pid {pid}) is running for this data directory; \
                     use `ambar workflow add {}` to run it in the background",
                        file.display()
                    ),
                    other => other.into(),
                })?;
            let notifier: Arc<dyn Notifier> = match notify {
                NotifyTarget::Desktop => Arc::new(DesktopNotifier),
                NotifyTarget::Log => Arc::new(LogNotifier),
            };
            let engine = Engine::new(
                open_store(&dir)?,
                notifier,
                Arc::new(TerminalApprover),
                EngineConfig::new(&dir),
            )?;
            describe(&wf);
            println!();
            let id = wf.id.clone();
            let is_manual = matches!(wf.trigger, Trigger::Manual);
            let is_files = wf.trigger.provides_file();
            engine.register(wf)?;

            if !*yes && !ask("Grant these permissions? [y/N] ") {
                bail!("permissions not granted; nothing was run");
            }
            if *allow_sensitive {
                eprintln!("note: sensitive actions are pre-approved for `{id}`");
            }
            engine.grant_permissions(&id, *allow_sensitive)?;
            engine.activate(&id)?;

            if is_manual || (*once && !is_files) {
                let r = engine.run_now(&id).await?;
                print_run(&r, true);
                engine.cancel(&id)?;
                if !matches!(r.status, ambar_automation::ExecutionStatus::Succeeded) {
                    std::process::exit(1);
                }
                return Ok(());
            }
            if *once {
                bail!("--once only applies to scheduled workflows");
            }

            println!("Running `{id}`. Press Ctrl-C to stop.");
            let mut events = engine.subscribe();
            loop {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => break,
                    ev = events.recv() => match ev {
                        Ok(EngineEvent::ExecutionFinished(r)) => print_run(&r, true),
                        Ok(_) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(_) => break,
                    }
                }
            }
            engine.cancel(&id)?;
            println!("\nstopped.");
        }
        Command::History {
            workflow,
            limit,
            steps,
        } => {
            let store = open_store(&data_dir(&cli)?)?;
            let runs = store.executions(workflow.as_deref(), *limit)?;
            if runs.is_empty() {
                println!("No executions recorded yet.");
            }
            for r in &runs {
                print_run(r, *steps);
            }
        }
        Command::Audit { workflow } => {
            let store = open_store(&data_dir(&cli)?)?;
            for a in store.audit_entries(workflow.as_deref())? {
                let when =
                    a.at.with_timezone(&chrono::Local)
                        .format("%Y-%m-%d %H:%M:%S");
                println!("{when}  {:<20} {:<10} {}", a.workflow_id, a.event, a.detail);
            }
        }
        Command::Workflows => {
            let store = open_store(&data_dir(&cli)?)?;
            for (id, _, state) in store.list_workflows()? {
                println!("{id:<24} {state}");
            }
        }
        Command::Service(cmd) => {
            let code = service_cmds::service(cmd, &ServicePaths::new(data_dir(&cli)?)).await?;
            std::process::exit(code);
        }
        Command::Workflow(cmd) => {
            let code = service_cmds::workflow(cmd, &ServicePaths::new(data_dir(&cli)?)).await?;
            std::process::exit(code);
        }
        Command::Pkg(cmd) => {
            let code = pkg_cmds::pkg(cmd, &ServicePaths::new(data_dir(&cli)?)).await?;
            std::process::exit(code);
        }
        Command::Approval(cmd) => {
            let code = agent_cmds::approval(cmd, &ServicePaths::new(data_dir(&cli)?)).await?;
            std::process::exit(code);
        }
        Command::Agent(cmd) => {
            let code = agent_cmds::agent(cmd, &ServicePaths::new(data_dir(&cli)?)).await?;
            std::process::exit(code);
        }
        Command::Ai(cmd) => {
            let code = ai_cmds::ai(cmd, &ServicePaths::new(data_dir(&cli)?)).await?;
            std::process::exit(code);
        }
        Command::Revoke { workflow } => {
            let store = open_store(&data_dir(&cli)?)?;
            if store.revoke_grant(workflow)? {
                store.audit(workflow, "revoked", "via CLI")?;
                println!("Revoked permissions for `{workflow}`.");
            } else {
                println!("`{workflow}` had no grant.");
            }
        }
    }
    Ok(())
}
