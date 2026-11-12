# FAQ

### Is Ambar OS ready for everyday use?

Not yet. The Ambar software is built and has an extensive test suite, but the OS hasn't been booted on physical hardware. [Status](STATUS.md) lists exactly what is and isn't proven.

### What is it built on?

**Ubuntu 26.04 LTS** with the **KDE Plasma 6** desktop. Ambar adds:

- a macOS-style design: menu bar, Dock, Launchpad, Control Center and window buttons
- its own apps and defaults
- the automation layer

Ubuntu tools such as `apt` keep working, and Ubuntu's security updates apply until 2031. Ambar OS is not affiliated with or endorsed by Canonical or KDE.

### How do I install Ambar OS?

Download `ambar-os-0.2-amd64.iso` from the [Ambar OS ISO workflow](live-usb.md#download-the-iso), unzip it, and write that `.iso` to a USB stick or open it in a virtual machine. The steps are in [Try it from a USB stick](live-usb.md) and [Install](install.md). There is no ISO in the source folder, and GitHub keeps each build for 14 days.

The Security edition is a second ISO, `ambar-os-security-0.2-amd64.iso`, from the same workflow when the security job succeeded. The experimental Fedora image is a different product; its notes are on the [OS image](os-image.md#atomic-edition-experimental) page.

### How do I install apps?

Almost any way you like: the **Ambar Store**, a `.deb` from a website (double-click it), Flathub, Snap, AppImage, Windows programs through Wine, and `apt` in Terminal. See [Install apps](first-steps.md#install-apps).

### Can automations run commands or scripts?

No. Automations can only do the built-in actions: create folders, copy, move, write and trash files, and send notifications. That's a deliberate security choice. Packages can't add new actions either.

### Does the AI see my files?

The AI only sees the description you type. For suggestions, it also sees your existing automations' names, trigger types and run counts. It never runs anything. It writes a draft, which you review and approve. You can use a local model (Ollama) so nothing leaves your computer.

### Do I need an AI account?

No. Everything except "describe an automation" works without one, and a local Ollama model works without any account.

### What's an agent?

An agent is a program, often an AI assistant, that you give an identity and limited permissions, such as "read" and "propose". It can suggest automations, but it can never approve them, grant access or change the service. See the [Developer guide](developer-guide.md#writing-an-agent).

### How is it different from Kali Linux?

Kali is a toolkit for security testing: you run attack and analysis tools by hand. Ambar OS is an everyday desktop that runs safe automations for you. Ambar OS keeps system snapshots with Timeshift, so you can go back after a bad update. If you want Kali's tools on Ambar OS, run them in a container:

```sh
distrobox create --name kali --image docker.io/kalilinux/kali-rolling
distrobox enter kali
```

`distrobox` is installed.

### Does it run on a Mac?

Intel Macs can boot the ISO. On Apple silicon, use a VM ([how](install.md#virtual-machine)). The `ambar` command and the service also run on macOS itself. That port is untested.

### Where is my data?

| What | Where |
|---|---|
| Automations, history, audit log, approvals | `~/.local/share/ambar` |
| AI settings and keys (keys in a private `0600` file) | `~/.config/ambar` |

None of it is touched by OS updates or rollbacks.
