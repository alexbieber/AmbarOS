# First steps

Ten minutes from first login to your first automation.

## 1. The Welcome setup

The first time you log in, **Welcome to Ambar** opens. It has four steps:

1. **Automation service:** it checks that `ambard` (the service that runs automations) is running. It starts with your session, so this is usually already green.
2. **Appearance:** Light or Dark. This applies to the whole desktop.
3. **AI assistant (optional):** choose Claude, an OpenAI-compatible service, or a local Ollama model, and enter an API key if it needs one. You can skip this; everything else works without AI.
4. **Starter automations:** signed, ready-made automations. Each one shows exactly what it can access before you install it.

You can come back to it from the **Setup** page in the Automation Center.

## 2. The Automation Center

Open **Automation Center** from the launcher. Its pages:

| Page | What it's for |
|---|---|
| Overview | What's running, what's waiting, and a box to describe a new automation |
| Automations | Turn automations on and off, pause them, run them now, remove them |
| Proposals | Automations suggested by agents or the AI, waiting for your decision |
| Agents | Assistants that have limited access to Ambar |
| Activity | Every run, step by step |
| Setup | The Welcome steps again |

## 3. Make your first automation

### With AI

On **Overview**, type what you want, for example:

> When a PDF lands in Downloads, move it to Documents/PDFs and tell me

Then choose **Create draft**. Before anything is added, you see three things:

- the automation itself
- the folders it needs
- a **risk review**

Approve it, and it's added and switched on.

### By hand

Save this as `tidy-screenshots.toml`:

```toml
id = "tidy-screenshots"
name = "Tidy screenshots"

[trigger]
type = "folder_watch"
path = "~/Pictures/Screenshots"

[[conditions]]
type = "file_extension"
any_of = ["png"]

[[actions]]
type = "move_file"
to_dir = "~/Pictures/Screenshots/{date}"

[permissions]
write = ["~/Pictures/Screenshots"]
```

Then:

```sh
ambar validate tidy-screenshots.toml          # check it, and see what access it needs
ambar workflow add tidy-screenshots.toml --enable
```

All triggers, conditions and actions are in the [workflow format](workflow-format.md).

## 4. Search with KRunner

Press **Alt+Space** and type:

| Type | What happens |
|---|---|
| the name of an automation | run it now, or open it in the Automation Center |
| `automate` followed by a description | opens a new AI draft |
| `approvals` | shows how many actions are waiting for your decision |

## 5. Approvals

Moving files to the trash and overwriting files are sensitive. Unless you allowed them when you granted the automation its access, they need your decision each time. When one comes up, a notification appears. You can answer it in any of three places:

- the Automation Center
- KRunner
- a terminal:

```sh
ambar approval list
ambar approval allow <id>
ambar approval deny <id>
```

If nobody answers within 5 minutes, the action is **denied**.

## 6. See what happened

```sh
ambar service status          # is the service healthy?
ambar workflow list           # your automations and their state
ambar history --steps         # recent runs, step by step
ambar audit                   # every grant, change and refusal
```

Next: [Troubleshooting](troubleshooting.md) if something doesn't work, or the [Developer guide](developer-guide.md) to build agents and packages.
