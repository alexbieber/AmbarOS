# Security policy

Ambar OS runs automations and AI-generated workflows on people's own files, so security reports get priority.

## Report a vulnerability

**Don't open a public issue.** Report it privately through GitHub: *Security* → [*Report a vulnerability*](https://github.com/alexbieber/AmbarOS/security/advisories/new).

Please include:

- what an attacker can do, and what they need first (for example a local account, a malicious package, or a model response)
- steps to reproduce, or a test case
- the version (`ambar --version`, `sudo bootc status`)

We aim to reply within 7 days and to fix confirmed issues before disclosing them. You'll be credited in the advisory unless you'd rather not be.

## In scope

- **Permission bypass:** an automation, package or agent acting outside its grant, including symlink, `..` and race attacks on paths.
- **Execution:** any way to make Ambar run a program or shell command.
- **Service access:** `ambard` IPC authentication, agent scopes, owner-only operations, approvals.
- **Packages:** signature and trust-store bypass, and installing files outside a package's manifest.
- **AI:** a model response that leads to something running or being saved without the user's approval.
- **The OS image:** the live USB, the installer and its disk checks, image signing and updates.

## Known limitations

These are documented, not secret: see [`docs/STATUS.md`](docs/STATUS.md).

- A same-user attacker could race the path check.
- `ambard` has no SELinux confinement yet.
- Installed systems don't yet *require* signed images.

Reports that improve on these are still welcome.
