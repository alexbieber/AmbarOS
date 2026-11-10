//! `ambar ai …`: the natural-language automation builder.

use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use ambar_ai::provider::{CompletionRequest, Message};
use ambar_ai::{
    AiConfig, Draft, GenerateOptions, HttpProvider, LlmProvider, ProviderKind, RiskLevel,
    ScriptedProvider, SuggestContext, generate, suggest,
};
use ambar_automation::ValidationOptions;
use ambar_service::protocol::GrantSpec;
use ambar_service::{Client, ServicePaths};
use anyhow::{Context, Result, bail};
use clap::{Subcommand, ValueEnum};

#[derive(Subcommand, Debug)]
pub enum AiCmd {
    /// Show or change the model configuration.
    Config {
        /// Provider API.
        #[arg(long, value_enum)]
        provider: Option<ProviderArg>,
        /// Model name.
        #[arg(long)]
        model: Option<String>,
        /// Endpoint (https://…, or http://localhost for local servers).
        #[arg(long)]
        base_url: Option<String>,
        /// Environment variable that holds the API key.
        #[arg(long)]
        api_key_env: Option<String>,
    },
    /// Store an API key (read from stdin) in a private 0600 file.
    SetKey,
    /// Send a one-line test request to the configured model.
    Test,
    /// Describe an automation in plain words; get a workflow to review.
    Create {
        /// What the automation should do.
        description: Vec<String>,
        /// Where to save the workflow (default: ./<id>.toml).
        #[arg(long, short)]
        output: Option<PathBuf>,
        /// After saving, add it to the running service (asks for permission consent).
        #[arg(long)]
        add: bool,
        /// With --add: enable it right away.
        #[arg(long)]
        enable: bool,
        /// Skip confirmations (refused when the review finds high risks).
        #[arg(long)]
        yes: bool,
        /// Print the draft as JSON and save nothing (for the desktop app and scripts).
        #[arg(long)]
        json: bool,
    },
    /// Ask the model for automation ideas based on your existing workflows.
    Suggest {
        /// Something you want help with, in your words.
        #[arg(long)]
        goal: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ProviderArg {
    Anthropic,
    Openai,
    Ollama,
}

impl From<ProviderArg> for ProviderKind {
    fn from(p: ProviderArg) -> Self {
        match p {
            ProviderArg::Anthropic => ProviderKind::Anthropic,
            ProviderArg::Openai => ProviderKind::Openai,
            ProviderArg::Ollama => ProviderKind::Ollama,
        }
    }
}

fn config_path() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("AMBAR_AI_CONFIG") {
        return Ok(PathBuf::from(p));
    }
    AiConfig::default_path().context("HOME is not set")
}

fn secrets_file(paths: &ServicePaths) -> PathBuf {
    paths.data_dir.join("secrets/ai_api_key")
}

fn provider(paths: &ServicePaths) -> Result<(Box<dyn LlmProvider>, AiConfig)> {
    // Offline/demo mode: AMBAR_AI_SCRIPT=<file> replays canned replies separated by
    // a line containing only `---`. Used by tests; never contacts a network.
    if let Some(script) = std::env::var_os("AMBAR_AI_SCRIPT") {
        let text = std::fs::read_to_string(&script).context("reading AMBAR_AI_SCRIPT")?;
        let replies: Vec<String> = text.split("\n---\n").map(String::from).collect();
        return Ok((
            Box::new(ScriptedProvider::new(replies)),
            AiConfig::new(ProviderKind::Ollama),
        ));
    }
    let cfg = AiConfig::load(&config_path()?)?;
    let key = cfg.api_key(Some(&secrets_file(paths)))?;
    Ok((Box::new(HttpProvider::new(&cfg, key)?), cfg))
}

fn confirm(prompt: &str) -> bool {
    if !io::stdin().is_terminal() {
        return false;
    }
    crate::ask(prompt)
}

/// High-risk drafts need the user to type the workflow id.
fn confirm_high_risk(id: &str) -> bool {
    if !io::stdin().is_terminal() {
        return false;
    }
    eprint!("This draft has HIGH risks. Type the workflow id (`{id}`) to continue: ");
    let _ = io::stderr().flush();
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line).is_ok() && line.trim() == id
}

fn print_draft(d: &Draft) {
    println!(
        "\n──── draft ({} attempt{}) ────",
        d.attempts,
        if d.attempts == 1 { "" } else { "s" }
    );
    println!("{}", d.toml.trim());
    println!("──────────────────────────\n");
    crate::describe(&d.workflow);
    if d.risks.is_empty() {
        println!("  Review:   no risks found");
    } else {
        println!("  Review:");
        for r in &d.risks {
            let tag = match r.level {
                RiskLevel::High => "HIGH   ",
                RiskLevel::Warning => "warning",
                RiskLevel::Info => "info   ",
            };
            println!("    {tag}  {}", r.message);
        }
    }
    println!();
}

pub async fn ai(cmd: &AiCmd, paths: &ServicePaths) -> Result<i32> {
    match cmd {
        AiCmd::Config {
            provider,
            model,
            base_url,
            api_key_env,
        } => {
            let path = config_path()?;
            let mut cfg = match (AiConfig::load(&path), provider) {
                (Ok(c), None) => c,
                (Ok(mut c), Some(p)) => {
                    c.provider = (*p).into();
                    c
                }
                (Err(_), Some(p)) => AiConfig::new((*p).into()),
                (Err(e), None) => bail!("{e}"),
            };
            let changed = model.is_some()
                || base_url.is_some()
                || api_key_env.is_some()
                || provider.is_some();
            if let Some(m) = model {
                cfg.model = Some(m.clone());
            }
            if let Some(b) = base_url {
                cfg.base_url = Some(b.clone());
            }
            if let Some(k) = api_key_env {
                cfg.api_key_env = Some(k.clone());
            }
            cfg.base_url()?; // reject insecure endpoints before saving
            if changed {
                cfg.save(&path)?;
                println!("saved {}", path.display());
            }
            println!("provider   {}", cfg.provider);
            println!(
                "model      {}",
                cfg.model().unwrap_or_else(|e| e.to_string())
            );
            println!("endpoint   {}", cfg.base_url()?);
            let key = match cfg.api_key(Some(&secrets_file(paths))) {
                Ok(Some(_)) => "configured".to_string(),
                Ok(None) => "not needed".into(),
                Err(e) => e.to_string(),
            };
            println!("api key    {key}");
            if cfg.provider != ProviderKind::Ollama {
                println!(
                    "\nYour descriptions and the workflow format are sent to {}.",
                    cfg.base_url()?
                );
                println!(
                    "File contents and names are never sent. Use --provider ollama to stay local."
                );
            }
            Ok(0)
        }
        AiCmd::SetKey => {
            eprintln!("Paste the API key and press Enter:");
            let mut key = String::new();
            io::stdin().lock().read_line(&mut key)?;
            if key.trim().is_empty() {
                bail!("no key entered");
            }
            let f = secrets_file(paths);
            ambar_service::paths::ensure_private_dir(&paths.data_dir)?;
            ambar_service::paths::ensure_private_dir(&paths.data_dir.join("secrets"))?;
            ambar_ai::config::store_key(&f, &key)?;
            println!("stored in {} (mode 0600)", f.display());
            Ok(0)
        }
        AiCmd::Test => {
            let (p, _) = provider(paths)?;
            let reply = tokio::time::timeout(
                Duration::from_secs(60),
                p.complete(&CompletionRequest {
                    system: "Reply with the single word: ready".into(),
                    messages: vec![Message::user("Are you there?")],
                    max_tokens: 10,
                }),
            )
            .await
            .context("the model did not answer within 60s")??;
            println!("{}: {}", p.describe(), reply.trim());
            Ok(0)
        }
        AiCmd::Create {
            description,
            output,
            add,
            enable,
            yes,
            json,
        } => {
            let description = description.join(" ");
            let (p, cfg) = provider(paths)?;
            eprintln!("asking {} …", p.describe());
            let opts = GenerateOptions {
                max_attempts: cfg.max_attempts,
                max_tokens: cfg.max_tokens,
                validation: ValidationOptions::default(),
                ..Default::default()
            };
            let draft = generate(p.as_ref(), &description, &opts).await?;
            if *json {
                let wf = &draft.workflow;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "id": wf.id,
                        "name": wf.name,
                        "trigger": wf.trigger.kind(),
                        "toml": draft.toml,
                        "attempts": draft.attempts,
                        "risks": draft.risks,
                        "permissions": {
                            "read": wf.permissions.read,
                            "write": wf.permissions.write,
                            "notify": wf.permissions.notify,
                        },
                    }))?
                );
                return Ok(0);
            }
            print_draft(&draft);

            let high = draft.risks.iter().any(|r| r.level == RiskLevel::High);
            if high && !confirm_high_risk(&draft.workflow.id) {
                bail!(
                    "not saved: the draft has high risks. Edit the description or the TOML and try again."
                );
            }
            let out = output
                .clone()
                .unwrap_or_else(|| PathBuf::from(format!("{}.toml", draft.workflow.id)));
            if !*yes && !high && !confirm(&format!("Save to {}? [y/N] ", out.display())) {
                println!("not saved");
                return Ok(1);
            }
            write_new(&out, &draft)?;
            println!(
                "saved {} — edit it freely; it is validated again when added",
                out.display()
            );

            if *add {
                let mut c = Client::connect(paths, "ambar-ai")
                    .await
                    .context("ambard is not running (start it with `ambar service start`)")?;
                if !*yes
                    && !high
                    && !confirm("Grant the permissions above and add it to the service? [y/N] ")
                {
                    println!(
                        "not added; run `ambar workflow add {}` when ready",
                        out.display()
                    );
                    return Ok(0);
                }
                let info = c
                    .register(
                        &std::fs::read_to_string(&out)?,
                        Some(GrantSpec {
                            allow_sensitive: false,
                        }),
                        false,
                    )
                    .await?;
                if *enable {
                    c.enable(&info.id).await?;
                    println!("added and enabled `{}`", info.id);
                } else {
                    println!(
                        "added `{}` (disabled); enable with `ambar workflow enable {}`",
                        info.id, info.id
                    );
                }
            }
            Ok(0)
        }
        AiCmd::Suggest { goal } => {
            let (p, cfg) = provider(paths)?;
            let mut ctx = SuggestContext {
                goal: goal.clone(),
                ..Default::default()
            };
            if let Ok(store) = ambar_automation::Store::open(&paths.db()) {
                for (id, def, _) in store.list_workflows().unwrap_or_default() {
                    let runs = store.executions(Some(&id), 1000).unwrap_or_default();
                    let failed = runs
                        .iter()
                        .filter(|r| r.status.as_str() == "failed")
                        .count();
                    let (name, trigger) = ambar_automation::Workflow::from_toml(&def)
                        .map(|w| (w.name, w.trigger.kind().to_string()))
                        .unwrap_or((id, "?".into()));
                    ctx.workflows.push((name, trigger, runs.len(), failed));
                }
            }
            eprintln!("asking {} …", p.describe());
            let ideas = suggest(p.as_ref(), &ctx, cfg.max_tokens).await?;
            for (i, s) in ideas.iter().enumerate() {
                println!(
                    "{}. {}\n   {}\n   ambar ai create \"{}\"\n",
                    i + 1,
                    s.title,
                    s.description,
                    s.description.replace('"', "'")
                );
            }
            Ok(0)
        }
    }
}

fn write_new(path: &Path, draft: &Draft) -> Result<()> {
    if path.exists() {
        bail!(
            "{} already exists; pass --output to choose another file",
            path.display()
        );
    }
    let header = "# Generated by `ambar ai create`. Review before adding.\n";
    std::fs::write(path, format!("{header}{}\n", draft.toml.trim()))
        .with_context(|| format!("writing {}", path.display()))
}
