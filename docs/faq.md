# FAQ

### Is Ambar OS ready for everyday use?

Not yet. The Ambar software is built and has an extensive test suite, but the OS hasn't been booted on physical hardware. [Status](STATUS.md) lists exactly what is and isn't proven.

### Is it a Fedora remix?

It's built on Fedora's KDE atomic desktop (Kinoite). Ambar adds its own software, desktop look and defaults on top. Fedora tools such as `dnf`, `toolbox` and `bootc` keep working. Ambar OS is not affiliated with or endorsed by the Fedora Project.

### How do I install apps?

Install desktop apps from **Discover**, which installs Flatpak apps. If Flathub isn't listed, enable it in Discover's settings. For command-line tools, use a **toolbox** (`toolbox create`, then `toolbox enter`) and `dnf install` inside it. The base system is read-only on purpose, which is what makes updates safe to undo.

### Can automations run commands or scripts?

No. Automations can only do the built-in actions: create folders, copy, move, write and trash files, and send notifications. That's a deliberate security choice. Packages can't add new actions either.

### Does the AI see my files?

The AI only sees the description you type. For suggestions, it also sees your existing automations' names, trigger types and run counts. It never runs anything. It writes a draft, which you review and approve. You can use a local model (Ollama) so nothing leaves your computer.

### Do I need an AI account?

No. Everything except "describe an automation" works without one, and a local Ollama model works without any account.

### What's an agent?

An agent is a program, often an AI assistant, that you give an identity and limited permissions, such as "read" and "propose". It can suggest automations, but it can never approve them, grant access or change the service. See the [Developer guide](developer-guide.md#writing-an-agent).

### How is it different from Kali Linux?

Kali is a toolkit for security testing: you run attack and analysis tools by hand. Ambar OS is an everyday desktop that runs safe automations for you. Ambar OS has atomic updates with rollback; Kali updates package by package. If you want Kali's tools on Ambar OS, run them in a container:

```sh
distrobox create --name kali --image docker.io/kalilinux/kali-rolling
distrobox enter kali
```

The `distrobox` tool isn't included in the image; get it with `toolbox` or from Fedora's packages.

### Does it run on a Mac?

Not directly on Apple silicon. Use a VM in UTM with the ARM64 image ([how](install.md#virtual-machine)). The `ambar` command and the service also run on macOS itself. That port is untested.

### Where is my data?

| What | Where |
|---|---|
| Automations, history, audit log, approvals | `~/.local/share/ambar` |
| AI settings and keys (keys in a private `0600` file) | `~/.config/ambar` |

None of it is touched by OS updates or rollbacks.
