# Workflow format

Workflows are TOML files. Unknown fields and unknown action types are rejected
at parse time, so typos fail loudly instead of being ignored.

## Top level

| Field | Required | Description |
|---|---|---|
| `id` | yes | 1–64 chars: lowercase letters, digits, `-` |
| `name` | yes | Display name |
| `description` | no | Free text |
| `[trigger]` | yes | What starts the workflow |
| `[[conditions]]` | no | All must pass (folder triggers only) |
| `[[actions]]` | yes | 1–50 steps, run in order; the first failure stops the run |
| `[permissions]` | no | `read`, `write` (folder lists), `notify` (bool) |

Paths must be absolute or start with `~/`, and must not contain `..`.

## Triggers

```toml
[trigger]
type = "manual"
```

```toml
[trigger]
type = "schedule"
every = "15m"              # humantime: 30s, 15m, 1h, 1day … (minimum 10s)
# or
cron = "0 9 * * 1-5"       # five-field cron, local time
```

```toml
[trigger]
type = "folder_watch"
path = "~/Downloads"
events = ["created"]       # created | modified | removed (default: created)
pattern = "*.pdf"          # optional glob on the file name
recursive = false          # default false
```

## Conditions (folder triggers)

```toml
[[conditions]]
type = "file_extension"
any_of = ["pdf", "png"]

[[conditions]]
type = "file_name_matches"
glob = "invoice-*"

[[conditions]]
type = "file_size"
min_bytes = 1
max_bytes = 10_000_000
```

## Actions

| Type | Fields | Permission | Sensitive? |
|---|---|---|---|
| `create_directory` | `path` | write `path` | no |
| `copy_file` | `from` (default `{file}`), `to_dir`, `overwrite` | read `from`, write `to_dir` | if `overwrite` |
| `move_file` | `from` (default `{file}`), `to_dir`, `overwrite` | write both | if `overwrite` |
| `write_file` | `path`, `content`, `append` | write `path` | if it replaces an existing file |
| `trash_file` | `path` (default `{file}`) | write `path` | always |
| `notify` | `title`, `body` | `notify = true` | no |

`copy_file` and `move_file` keep the file name and create `to_dir` if needed.
They refuse to replace an existing file unless `overwrite = true`, which needs
approval. `trash_file` moves the file into the engine's trash folder
(`<data-dir>/trash`); it never deletes.

## Template variables

Usable in any path or text field:

| Variable | Available | Example |
|---|---|---|
| `{date}` | always | `2026-11-11` |
| `{time}` | always | `14-05-09` |
| `{datetime}` | always | `2026-11-11T14-05-09` |
| `{workflow_id}`, `{execution_id}` | always | |
| `{file}` | folder triggers | `/home/a/Downloads/x.pdf` |
| `{file_name}`, `{file_stem}`, `{file_ext}` | folder triggers | `x.pdf`, `x`, `pdf` |
| `{file_dir}` | folder triggers | `/home/a/Downloads` |
| `{event}` | folder triggers | `created` |

Unknown variables, and file variables in non-file workflows, are validation
errors.
