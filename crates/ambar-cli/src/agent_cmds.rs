//! `ambar agent …`: agent identities and their proposals.

use ambar_service::{Client, ServicePaths};
use anyhow::{Context, Result, bail};
use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum ApprovalCmd {
    /// Sensitive actions waiting for your decision.
    List,
    /// Let a waiting action run.
    Allow {
        /// Approval id.
        id: String,
    },
    /// Refuse a waiting action.
    Deny {
        /// Approval id.
        id: String,
    },
}

pub async fn approval(cmd: &ApprovalCmd, paths: &ServicePaths) -> Result<i32> {
    let mut c = connect(paths).await?;
    match cmd {
        ApprovalCmd::List => {
            let list = c.approvals().await?;
            if list.is_empty() {
                println!("Nothing is waiting for your approval.");
            }
            for a in list {
                println!("{}  {} wants to {}
  (auto-denied at {})", a.id, a.workflow_id, a.description, a.expires_at);
            }
        }
        ApprovalCmd::Allow { id } => {
            let a = c.decide(id, true).await?;
            println!("allowed: {} may {}", a.workflow_id, a.description);
        }
        ApprovalCmd::Deny { id } => {
            let a = c.decide(id, false).await?;
            println!("denied: {} may not {}", a.workflow_id, a.description);
        }
    }
    Ok(0)
}

#[derive(Subcommand, Debug)]
pub enum AgentCmd {
    /// Create an agent identity and print its token (shown only once).
    Create {
        /// Unique name.
        name: String,
        /// Scope to grant (repeatable): read, propose, memory, run:<id|*>, control:<id|*>.
        #[arg(long = "scope", required = true)]
        scopes: Vec<String>,
        /// What the agent is for.
        #[arg(long, default_value = "")]
        description: String,
    },
    /// List agents with their scopes and activity.
    List,
    /// Revoke an agent; its token stops working immediately.
    Revoke {
        /// Agent id or name.
        agent: String,
    },
    /// Show workflow proposals from agents.
    Proposals {
        /// Include decided proposals.
        #[arg(long)]
        all: bool,
    },
    /// Approve a proposal: adds the workflow and grants its permissions.
    Approve {
        /// Proposal id.
        id: String,
        /// Enable it right away.
        #[arg(long)]
        enable: bool,
        /// Pre-approve sensitive actions.
        #[arg(long)]
        allow_sensitive: bool,
        /// Skip the confirmation (refused for high-risk proposals).
        #[arg(long)]
        yes: bool,
    },
    /// Reject a proposal.
    Reject {
        /// Proposal id.
        id: String,
    },
}

async fn connect(paths: &ServicePaths) -> Result<Client> {
    Client::connect(paths, "ambar-cli")
        .await
        .context("ambard is not running (start it with `ambar service start`)")
}

pub async fn agent(cmd: &AgentCmd, paths: &ServicePaths) -> Result<i32> {
    let mut c = connect(paths).await?;
    match cmd {
        AgentCmd::Create {
            name,
            scopes,
            description,
        } => {
            let scopes: Vec<&str> = scopes.iter().map(String::as_str).collect();
            let (agent, token) = c.agent_create(name, description, &scopes).await?;
            println!("created {} ({})", agent.name, agent.id);
            println!("scopes  {}", scopes.join(", "));
            println!("\ntoken (shown once — store it in the agent's secret store):\n{token}");
        }
        AgentCmd::List => {
            let agents = c.agent_list().await?;
            if agents.is_empty() {
                println!("No agents. Create one with `ambar agent create <name> --scope read`.");
            }
            println!(
                "{:<14} {:<18} {:<8} {:>8} {:>7}  {:<20} SCOPES",
                "ID", "NAME", "STATUS", "REQUESTS", "DENIED", "LAST SEEN"
            );
            for a in agents {
                let scopes: Vec<String> = a.scopes.iter().map(ToString::to_string).collect();
                let seen = a
                    .last_seen
                    .as_deref()
                    .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
                    .map(|t| {
                        t.with_timezone(&chrono::Local)
                            .format("%Y-%m-%d %H:%M")
                            .to_string()
                    })
                    .unwrap_or_else(|| "never".into());
                println!(
                    "{:<14} {:<18} {:<8} {:>8} {:>7}  {:<20} {}",
                    a.id,
                    a.name,
                    if a.revoked { "revoked" } else { "active" },
                    a.requests,
                    a.denied,
                    seen,
                    scopes.join(",")
                );
            }
        }
        AgentCmd::Revoke { agent } => {
            let a = c.agent_revoke(agent).await?;
            println!("revoked {} ({})", a.name, a.id);
        }
        AgentCmd::Proposals { all } => {
            let ps = c.proposals().await?;
            let shown: Vec<_> = ps
                .iter()
                .filter(|p| *all || p.status == "pending")
                .collect();
            if shown.is_empty() {
                println!("No {}proposals.", if *all { "" } else { "pending " });
            }
            for p in shown {
                println!(
                    "{}  [{}]  workflow `{}` from {}",
                    p.id, p.status, p.workflow_id, p.agent_id
                );
                if !p.reason.is_empty() {
                    println!("  reason: {}", p.reason);
                }
                for r in &p.risks {
                    println!("  risk:   {r}");
                }
            }
        }
        AgentCmd::Approve {
            id,
            enable,
            allow_sensitive,
            yes,
        } => {
            let p = c
                .proposals()
                .await?
                .into_iter()
                .find(|p| &p.id == id)
                .with_context(|| format!("no proposal `{id}`"))?;
            let wf = ambar_automation::Workflow::from_toml(&p.definition)?;
            println!("Proposal {} from {}", p.id, p.agent_id);
            if !p.reason.is_empty() {
                println!("Reason: {}", p.reason);
            }
            println!("\n{}\n", p.definition.trim());
            crate::describe(&wf);
            for r in &p.risks {
                println!("  risk: {r}");
            }
            println!();
            let high = p.risks.iter().any(|r| r.starts_with("high"));
            if high && *yes {
                bail!("this proposal has high risks; approve it interactively");
            }
            if !*yes && !crate::ask("Approve and grant these permissions? [y/N] ") {
                bail!("not approved");
            }
            c.approve(id, *allow_sensitive, *enable).await?;
            println!(
                "approved: `{}` added{}",
                wf.id,
                if *enable {
                    " and enabled"
                } else {
                    " (disabled)"
                }
            );
        }
        AgentCmd::Reject { id } => {
            c.reject(id).await?;
            println!("rejected {id}");
        }
    }
    Ok(0)
}
