# Publish on GitHub

How to put Ambar OS on GitHub from Cursor, switch on the automatic ISO build, and publish a release. It takes about 20 minutes, plus the first ISO build (1–3 hours on GitHub's machines).

The repository is set up for **`github.com/alexbieber/AmbarOS`**. If you use another name, see [Using a different repository name](#using-a-different-repository-name).

## 1. Open the project in Cursor

1. Unzip `ambar-os.zip`. You get a folder called `ambar-os` that already contains the git history (a hidden `.git` folder, branch `main`).
2. In Cursor: **File → Open Folder…** and choose `ambar-os`.
3. Cursor reads **`AGENTS.md`** and the rules in **`.cursor/rules/`** automatically. They tell the AI what the project is, how it's built and tested, and the rules it must follow. You don't need to set anything up. To check, open **Cursor Settings → Rules**: the project rules are listed there.

!!! tip "Let Cursor do it"
    The repo has a step-by-step playbook for Cursor, [`CURSOR.md`](https://github.com/alexbieber/AmbarOS/blob/main/CURSOR.md). In Cursor's chat (Agent mode), type: *"Follow CURSOR.md and push the code to GitHub."* It runs the checks, pushes, and tells you which settings to switch on. The steps below are the same thing done by hand.

## 2. Check before the first push

Open Cursor's terminal (**Terminal → New Terminal**) and run:

```sh
git status                        # should say: nothing to commit, working tree clean
git branch --show-current         # should say: main (the workflows only run on main)
git ls-files | grep -E '^(output|target|site)/' || echo "OK: no build output tracked"
git ls-files -z | xargs -0 du -k | sort -rn | head -3    # biggest files, all a few MB
```

The ISO (3.6 GB) and build folders are in `.gitignore`, so they're never uploaded. GitHub refuses files over 100 MB; the biggest file here is under 3 MB.

## 3. Create the repository on GitHub

1. Go to **github.com/new** and sign in as `alexbieber`.
2. **Repository name:** `AmbarOS`.
3. **Description:** `A complete desktop OS designed like macOS: Ubuntu 26.04 LTS + KDE Plasma 6, install any app, automations built in.`
4. Choose **Public**. GitHub Actions on standard runners is free for public repositories. A private repository on the Free plan gets only 500 MB of artifact storage, which is less than one ISO.
5. **Leave everything else empty**: no README, no .gitignore, no licence. The project already has them, and an extra first commit would make the push fail.
6. Click **Create repository**.

## 4. Push from Cursor

**Option A: with the Source Control panel**

1. Open **Source Control** (the branch icon in the left bar, or Ctrl+Shift+G / Cmd+Shift+G).
2. Click **… → Remote → Add Remote…**, paste `https://github.com/alexbieber/AmbarOS.git` and name it `origin`.
3. Click **Publish Branch** (or **Sync Changes**). Sign in to GitHub in the browser when Cursor asks.

**Option B: in the terminal**

```sh
git remote add origin https://github.com/alexbieber/AmbarOS.git
git push -u origin main
```

If git asks for a password, use a **personal access token**, not your GitHub password. Create one at **GitHub → Settings → Developer settings → Personal access tokens → Fine-grained tokens**, with access to the `AmbarOS` repository and **Contents: Read and write** plus **Workflows: Read and write** (needed because the push includes `.github/workflows/`). You can also install the GitHub CLI and run `gh auth login` once.

## 5. Switch on the GitHub features

On the repository page, open **Settings**:

| Where | What to set | Why |
|---|---|---|
| **General → Features** | Tick **Discussions** and **Issues** | The docs link to them for help and hardware reports |
| **General → Social preview** | Upload `docs/images/social-preview.jpg` | The picture shown when the link is shared on X, LinkedIn or WhatsApp |
| **Pages** | **Source: GitHub Actions** | Publishes the docs site at `alexbieber.github.io/AmbarOS` |
| **Actions → General** | **Allow all actions**; Workflow permissions: **Read and write** | The workflows build, test and publish |
| **Branches → Add branch ruleset** (optional, later) | Target `main`, require the **CI** checks to pass | Stops broken changes reaching `main` |

On the repository's main page, click the ⚙️ next to **About** and add the website `https://alexbieber.github.io/AmbarOS/` and topics such as `linux`, `operating-system`, `ubuntu`, `kde-plasma`, `macos`, `desktop`, `automation`, `rust`.

## 6. Watch the workflows

Open the **Actions** tab. The push starts:

| Workflow | What it does | Time |
|---|---|---|
| **CI** | Rust format, lint and tests on Linux and macOS; Python and desktop tests | ~15 min |
| **Docs** | Builds the docs site and publishes it to GitHub Pages | ~3 min |
| **Ambar OS ISO** | Builds the ISO with Firefox, boots it with KVM, runs the boot test, keeps the ISO and screenshots | 1–3 h |
| **OS image** | The experimental atomic edition (Fedora Kinoite) | ~1 h |

A ❌ on the first run is normal for a project that has never run on GitHub before. Click the run, open the failed step, and paste the error into Cursor with *"This GitHub Actions step failed, fix it."* Push the fix and the workflow runs again.

**If the CI macOS job fails:** nothing has ever run on macOS (see [Status](STATUS.md)). That's useful to learn, but it doesn't block the ISO.

## 7. Get the ISO

When **Ambar OS ISO** turns green:

1. Open the run and scroll to **Artifacts**.
2. Download **ambar-os-iso** (the ISO and its `.sha256` file) and **ambar-os-boot-evidence** (serial log and screenshots of the booted desktop).
3. Artifacts are kept for **14 days**. Run the workflow again any time with **Actions → Ambar OS ISO → Run workflow**.

Check the download before using it:

```sh
sha256sum -c ambar-os-0.2-amd64.iso.sha256        # macOS: shasum -a 256 -c ambar-os-0.2-amd64.iso.sha256
```

Then try it in a virtual machine or on a USB stick: [Try it from a USB stick](live-usb.md).

## 8. Publish a release

GitHub releases accept files **under 2 GiB each**, and the ISO is 3.6 GB. Choose one:

=== "Split the ISO (simplest)"

    ```sh
    split -b 1900M -d ambar-os-0.2-amd64.iso ambar-os-0.2-amd64.iso.part
    sha256sum ambar-os-0.2-amd64.iso > ambar-os-0.2-amd64.iso.sha256
    ```

    Upload the two `.part00`/`.part01` files and the `.sha256` to the release. Put this in the release notes so people can join them:

    ```sh
    cat ambar-os-0.2-amd64.iso.part* > ambar-os-0.2-amd64.iso      # Windows: copy /b ambar-os-0.2-amd64.iso.part00+ambar-os-0.2-amd64.iso.part01 ambar-os-0.2-amd64.iso
    sha256sum -c ambar-os-0.2-amd64.iso.sha256
    ```

=== "Host the ISO elsewhere"

    Many Linux distributions host ISOs on **SourceForge** (free for open-source projects, no size limit, worldwide mirrors). Upload the ISO there and link to it from the GitHub release. Keep the `.sha256` on GitHub.

To create the release: **Releases → Draft a new release → Choose a tag:** `v0.2.0` → **Create new tag**. Title it `Ambar OS 0.2`, attach the files, and copy the notes from the template below.

??? note "Release notes template"
    ```markdown
    ## Ambar OS 0.2 (preview)

    A complete desktop OS designed like macOS, built on Ubuntu 26.04 LTS and KDE Plasma 6.

    **What works (tested in virtual machines):** live USB boot to the full desktop, the macOS-style
    shell (menu bar, Dock, Launchpad, Control Center, search, light/dark), 27+ apps, the Ambar Store,
    a full install to disk, and the installed system booting to its login screen.

    **Not proven yet:** physical PCs, Secure Boot on real firmware, Wi-Fi and sleep.
    See STATUS.md for the full list. Please send a hardware report: run `ambar-hwreport`.

    **Download:** join the parts (commands below), check the SHA-256, write it to a USB stick.
    ```

Mark it **pre-release** until it has been tested on real PCs.

## 9. Share it

- Pin the repository on your GitHub profile.
- Post the tour video (`docs/media/ambar-os-tour.mp4`) with the repository link. Videos get more reach on X than links.
- Ask testers to open a **Hardware report** issue: each one adds a row to the [hardware list](hardware.md).

## Using a different repository name

The name `alexbieber/AmbarOS` appears in the docs config, the README badges and some links. To change it, ask Cursor:

> *Replace every `alexbieber/AmbarOS` and `alexbieber.github.io/AmbarOS` in the repo with `<owner>/<repo>`, then run `mkdocs build --strict` and the tests in `tests/`.*

The workflows themselves don't depend on the name.

## Day-to-day after that

```sh
git pull                              # get changes (from GitHub or other machines)
# …edit in Cursor…
bash scripts/verify.sh                # or the quicker checks in AGENTS.md
git add -A && git commit -m "Short description of the change"
git push                              # CI runs; the ISO rebuilds when os/, desktop/ or crates/ change
```
