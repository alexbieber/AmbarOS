# Ambar OS

<p align="center"><img src="images/brand/ambar-mark.svg" alt="" width="140"></p>

**Designed for Humans. Built for AI. Open to Everyone.**

Ambar OS is a Linux operating system for people who want their computer to take care of the repetitive work. It's built on Ubuntu 26.04 LTS and KDE Plasma 6, designed to look and work like macOS. On top of that, it adds an automation service that runs your automations in the background, an app to manage them, and an optional AI assistant that writes new ones from a plain-language description.

![The Ambar OS desktop](images/os/desktop.jpg)

!!! warning "Early project"
    The Ambar software is built and tested, but **it has only been booted in virtual machines so far**. Read [Status](STATUS.md) before you rely on it.

## See it in action

<video controls preload="metadata" poster="media/ambar-demo-poster.jpg" style="width:100%;border-radius:12px">
  <source src="media/ambar-demo.mp4" type="video/mp4">
</video>

<small>A one-minute recording of the real Automation Center and `ambard`. The AI reply is replayed from a file, so the demo needs no API key.</small>

## Start here

| I want to… | Go to |
|---|---|
| Try it without installing anything | [Try it from a USB stick](live-usb.md) |
| Install it | [Install](install.md) |
| Learn the basics in 10 minutes | [First steps](first-steps.md) |
| Check whether my computer works | [Hardware](hardware.md) |
| Fix a problem | [Troubleshooting](troubleshooting.md) |
| Write automations by hand | [Workflow format](workflow-format.md) |
| Build agents or packages | [Developer guide](developer-guide.md) |
| Help out | [Community](community.md) |

## What makes it different

- **Automations, safely.** An automation can only use the built-in actions: create folders, copy, move, write or trash files, and send notifications. It can only touch the folders it declares. It can't run programs or shell commands. You approve its access before it runs, and everything it does goes into an audit log.
- **AI that proposes, never executes.** Describe what you want and the AI drafts an automation. You see the draft, the folders it needs and a risk review. Nothing runs until you approve it.
- **Updates you can undo.** The whole OS updates as one image. If an update goes wrong, restart and pick the previous version.
- **Your files stay yours.** The automation service runs as you, not as root, and listens only on a private local socket.

## How it fits together

```
 Automation Center · KRunner search · Welcome      ← what you see
            │  (local socket, owner token)
         ambard  ── automation engine ── audit log  ← runs as you, in the background
            │
 macOS-style Plasma 6 desktop · Ubuntu 26.04 LTS   ← the OS underneath
```

More detail: [Architecture](architecture.md) and [Service](service.md).
