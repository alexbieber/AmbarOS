# CURSOR.md: publish Ambar OS to GitHub

**For Cursor.** When the user says *"follow CURSOR.md"*, *"push the code"* or *"publish to GitHub"*, carry out the steps below in order. Run commands in Cursor's terminal. After each step, tell the user what happened in one line. **Stop and ask** wherever a step says so; never guess on those.

General rules for every step:

- Don't rewrite, regenerate or "improve" the README, docs or videos while publishing. They're finished and tested. Publishing only checks them and pushes them.
- Never force-push (`--force`), never rewrite history, never delete branches.
- Never put a token or password in a file, a command, a commit or a chat message. The user signs in themselves.
- If a command fails, show the user the exact error, explain it in one sentence, and propose a fix. Don't try random variations.

---

## Step 1: Check where we are

```sh
git rev-parse --show-toplevel        # must end in /ambar-os
git branch --show-current            # must print: main
git status --short                   # empty = nothing uncommitted
git log --oneline | head -5
git remote -v                        # empty, or origin → github.com/<owner>/AmbarOS
```

- **Not a git repository:** stop. The user unzipped something other than the provided `ambar-os.zip`, or deleted the hidden `.git` folder. Ask them to unzip `ambar-os.zip` again. Do **not** run `git init`: it throws away the project history.
- **Branch is `master`:** run `git branch -m master main`. All workflows trigger on `main` only.
- **Uncommitted changes:** show `git status` to the user and ask whether to commit them (Step 3) or leave them out.

## Step 2: Pre-push checks

Run all of these. Every one must pass before pushing.

```sh
# 1. No build output tracked (the ISO alone is 3.6 GB)
git ls-files | grep -E '^(output|target|site)/' && echo "FAIL: build output is tracked" || echo "OK: no build output"

# 2. No file near GitHub's 100 MB limit
git ls-files -z | xargs -0 du -k | sort -rn | head -5          # biggest should be a few MB

# 3. No secrets
git grep -nIE '(sk-ant-[A-Za-z0-9_-]{20,}|sk-[A-Za-z0-9]{32,}|ghp_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,}|AKIA[0-9A-Z]{16}|-----BEGIN [A-Z ]*PRIVATE KEY-----)' \
  -- ':!*.md' ':!tests/*' ':!crates/*/tests/*' && echo "FAIL: possible secret" || echo "OK: no secrets found"

# 4. The files the README and docs show are present
for f in README.md FEATURES.md AGENTS.md CURSOR.md LICENSE docs/STATUS.md docs/github-setup.md \
         docs/images/hero.jpg docs/images/social-preview.jpg \
         docs/media/ambar-os-tour.mp4 docs/media/ambar-os-tour.gif \
         docs/media/ambar-demo.mp4 docs/media/ambar-demo.gif; do
  [ -f "$f" ] && echo "OK  $f" || echo "MISSING  $f"
done
```

If Python 3 is installed, also run the quick tests. They take seconds and need no build:

```sh
python3 -m pip install --quiet pytest pyyaml 2>/dev/null
python3 -m pytest -q tests/test_agents.py os/tests/test_ubuntu.py
```

Any FAIL or MISSING: stop, show the user, and fix only that problem.

The full suite (`bash scripts/verify.sh`) needs Rust and PySide6 and takes longer. It also runs on GitHub after the push, so it's optional here.

## Step 3: Commit (only if Step 1 found changes the user wants)

```sh
git add -A
git status --short          # show the user the list first
git commit -m "<short imperative summary of the change>"
```

Commit messages are imperative ("Add …", "Fix …"), and the body says what was tested.

## Step 4: Connect to GitHub

**Ask the user first:** *"Have you created an empty repository on GitHub? Which owner/name: alexbieber/AmbarOS?"*

If it isn't created yet, tell the user to:

1. Open **https://github.com/new**.
2. Set the name to `AmbarOS` and choose **Public**. Actions are free for public repos; a private repo's 500 MB artifact quota can't hold the ISO.
3. Leave **README, .gitignore and licence all unticked**: the project has its own, and an extra first commit makes the push fail.
4. Click **Create repository**.

Then:

```sh
git remote -v | grep -q origin || git remote add origin https://github.com/alexbieber/AmbarOS.git
git remote -v
```

**If the user picked a different owner or name:** set the remote to that URL, then replace every `alexbieber/AmbarOS` and `alexbieber.github.io/AmbarOS` in the repo with the new values. Check with `git grep -n "alexbieber"`, then run `python3 -m pytest -q tests` and commit the change.

## Step 5: Push

```sh
git push -u origin main
```

Or use the Source Control panel: **… → Push** (or **Publish Branch**).

| Error | What to do |
|---|---|
| Asks for username/password, or `Authentication failed` | The user signs in. Easiest: Cursor's GitHub sign-in prompt, or `gh auth login` if the GitHub CLI is installed. Otherwise they create a fine-grained personal access token (GitHub → Settings → Developer settings) with **Contents: Read and write** and **Workflows: Read and write** on this repo, and paste it when git asks for the password. Never write the token anywhere. |
| `refusing to allow ... to create or update workflow` | The token is missing the **Workflows** permission. The user edits the token and adds it. |
| `rejected ... (fetch first)` / `non-fast-forward` | The GitHub repo isn't empty (it was created with a README or licence). **Ask the user** whether to delete and recreate the empty repo (simplest), or run `git pull --rebase origin main`, fix any conflicts, and push again. Never force-push. |
| `Repository not found` | Wrong URL or no access. Check `git remote -v` against the URL on GitHub. |

## Step 6: After the push, settings on GitHub

Tell the user to do these on the repository page (Cursor can't change GitHub settings):

| Setting | Value |
|---|---|
| **Settings → Pages → Source** | GitHub Actions (publishes the docs site) |
| **Settings → General → Features** | Tick **Issues** and **Discussions** |
| **Settings → General → Social preview** | Upload `docs/images/social-preview.jpg` |
| **Settings → Actions → General → Workflow permissions** | Read and write |
| **About (⚙️ on the main page)** | Description: *A complete desktop OS designed like macOS: Ubuntu 26.04 LTS + KDE Plasma 6, install any app, automations built in.* Website: `https://alexbieber.github.io/AmbarOS/`. Topics: `linux` `operating-system` `ubuntu` `kde-plasma` `macos` `desktop` `automation` `rust` |

Full detail: `docs/github-setup.md`.

## Step 7: Check the README and video render on GitHub

Open `https://github.com/<owner>/AmbarOS` and confirm with the user:

- **README.md** is the project's front page. It starts with the hero banner (`docs/images/hero.jpg`), then the badges, then the **tour GIF** (`docs/media/ambar-os-tour.gif`), which links to the MP4. **Don't replace it or generate a new README.** The CI and ISO badges show "no status" until their first workflow run finishes; that's normal.
- **Videos** in the repo, and where each one is used:

| File | What it is | Where it's used |
|---|---|---|
| `docs/media/ambar-os-tour.mp4` | 50-second tour of the OS, made only from real screenshots | Main video. Linked from the README top and FEATURES.md. Use it for X, LinkedIn and the release post. |
| `docs/media/ambar-os-tour.gif` | Silent looping version of the tour | Plays inline at the top of the README |
| `docs/media/ambar-demo.mp4` / `.gif` | 1-minute recording of the Automation Center: describe, review, approve | README "Ambar automation" section and the docs home page |
| `docs/media/ambar-os-tour-poster.jpg` | First frame of the tour | Thumbnail if a site needs one |

GitHub plays the MP4 when it's clicked in the file view. Inline in the README, the GIF is what plays. If the user wants the video to **play inline in the README with controls**, they can do it on github.com: edit `README.md`, drag `ambar-os-tour.mp4` into the editor, and GitHub uploads it and inserts a link that plays as a video. Cursor can't do this upload, because it happens in the browser.

To remake the tour after screenshots change: `python3 scripts/demo/os_tour.py` (needs Pillow and ffmpeg).

## Step 8: Watch the workflows

Tell the user to open the **Actions** tab. The push starts:

| Workflow | Result | Time |
|---|---|---|
| CI | Rust and Python tests on Linux and macOS | ~15 min |
| Docs | Builds and publishes the docs site | ~3 min |
| Ambar OS ISO | Builds the ISO with Firefox, boots it with KVM, uploads the ISO and screenshots | 1–3 h |
| OS image | Experimental atomic edition | ~1 h |

If one fails, the user pastes the failing step's log into Cursor. Then:

1. Read `AGENTS.md` (the project map and its rules).
2. Find the cause.
3. Make the smallest fix and add or adjust a test.
4. Commit and push. The workflow reruns on its own.

A failing macOS CI job is expected to need work: nothing has run on macOS yet (see `docs/STATUS.md`). It doesn't block the ISO.

## Step 9: The ISO and a release (only when the user asks)

- **Get the ISO:** Actions → the green *Ambar OS ISO* run → Artifacts → `ambar-os-iso`. Artifacts are kept 14 days.
- **Release:** a GitHub release file must be under 2 GiB and the ISO is 3.6 GB. Follow `docs/github-setup.md` → *Publish a release*: split the ISO into two parts, or host it on SourceForge. Tag `v0.2.0`, mark it **pre-release**, and use the notes template in that guide.

## Step 10: Report to the user

Finish with a short summary:

- the repository URL
- what was pushed (latest commit hash and message)
- which checks passed
- which GitHub settings are still to do
- that the ISO build is running and roughly when it should be done

---

## Day-to-day after the first push

```sh
git pull
# edit
python3 -m pytest -q tests os/tests     # quick checks; full: bash scripts/verify.sh
git add -A && git commit -m "<what changed>"
git push
```

Before changing code, read `AGENTS.md`. Its rules and the matching `.cursor/rules/` apply to every change.
