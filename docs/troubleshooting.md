# Troubleshooting

Find your problem below. Each fix starts with the quickest check. If nothing here helps, [ask for help](community.md#get-help), including what you tried and the output of the commands below.

## Starting the computer

### The USB stick won't boot

- **Written wrong.** Unzip the download first, then write `ambar-os-0.2-amd64.iso` as an image (balenaEtcher, Rufus in DD mode, Fedora Media Writer, or `dd`). Copying the zip onto the stick, or copying the ISO as a normal file, will not boot. See [Write the ISO to the stick](live-usb.md#1-write-the-iso-to-the-stick).
- **Boot menu.** Choose the entry that starts with **UEFI** in the boot menu.
- **Secure Boot.** Ambar boots through Ubuntu's signed shim. If the firmware still says *security violation*, turn Secure Boot off in the firmware settings, start Ambar, and report your model.
- **USB port.** Some machines only boot from USB 2 ports or from the left-hand ports. Try another port.

### Black or garbled screen after the boot menu

Restart and choose **Try or Install Ambar OS (safe graphics)**. This adds `nomodeset`. If that works, your graphics chip needs a driver fix. Please [send a hardware report](hardware.md#report-your-computer).

On an installed system, press **e** at the boot menu, add `nomodeset` to the end of the line that starts with `linux`, and press **Ctrl+X**.

### Something broke after an update

Open **Timeshift** (Launchpad → System), pick the snapshot from before the update, and choose **Restore**. If the desktop won't start at all, boot the USB stick, open Timeshift there, and restore your installed system from it. Your files in `/home` aren't touched by a system restore.

Timeshift needs to be set up once: open it, choose RSYNC, and pick a schedule.

## Automations

### "ambard is not running"

```sh
ambar service status                  # exit code 3 means not running
systemctl --user status ambard        # what systemd says
journalctl --user -u ambard -n 50     # its last 50 log lines
systemctl --user restart ambard
```

If it was turned off, `systemctl --user enable --now ambard` switches it back on for future logins.

### An automation didn't run

1. **Check it's switched on:** run `ambar workflow list`. The state must be `active`. If it's `paused`, run `ambar workflow resume <id>`.
2. **See what happened:** run `ambar history --steps`. Each run shows every step and why it failed.
3. **Check whether it was refused:** run `ambar audit`. A path outside its granted folders is refused and logged there. That's on purpose. Remove the automation, fix its `[permissions]`, and add it again so you can approve the new access.
4. **Folder automations:** they only react to files that arrive *after* they're switched on, and only to files matching their conditions (for example the extension).
5. **Scheduled automations:** runs missed while the computer was off or asleep are not caught up.

### An automation is waiting

It's probably waiting for your approval (trash or overwrite). Run `ambar approval list`, then `ambar approval allow <id>` or `ambar approval deny <id>`. Unanswered approvals are denied after 5 minutes.

### Undo a permission

```sh
ambar workflow remove <id>      # removes the automation and its grant; history is kept
```

## AI assistant

```sh
ambar ai config        # which provider, model and endpoint are set
ambar ai test          # sends one short request and shows the error, if any
```

| Message | Fix |
|---|---|
| authentication / 401 | Set the key again: `ambar ai set-key` (it reads the key from standard input). |
| connection refused (Ollama) | Start Ollama (`ollama serve`) and pull the model you set (`ollama pull <model>`). |
| "no draft" after several tries | The model kept proposing something Ambar doesn't allow, such as running a command. Describe the result you want with files and folders. |

API keys are never printed in errors or logs.

## Desktop

### Search (Alt+Space) shows no Ambar results

```sh
ambar service status                                    # the service must be running
busctl --user call os.ambar.KRunner /runner org.kde.krunner1 Match s "automate test"
```

If the second command fails, log out and back in so KRunner picks up the plugin.

### The Welcome window opens every time

It closes for good when you finish the last step. To open it again later, use the **Setup** page in the Automation Center.

## Installing

### The installer won't install next to Windows

- **BitLocker:** turn off BitLocker in Windows first, or the installer can't resize the Windows partition.
- **Fast Startup:** turn off Windows Fast Startup (Control Panel → Power Options).
- **Free space:** give Ambar OS at least 30 GB. Shrink the Windows partition in Windows' Disk Management, then choose **Install alongside** or **Manual**.

### The install stopped with an error

The installer keeps a log at `/root/.cache/calamares/session.log` while it runs. Copy it before you restart, and attach it to a [bug report](community.md#get-help).

## Collect information for a bug report

```sh
ambar --version
cat /etc/os-release
ambar service status
ambar history --steps | head -40
journalctl --user -u ambard -n 100 --no-pager
ambar-hwreport
```

Check the output before posting it, and remove anything you consider private, such as file names in your paths.
