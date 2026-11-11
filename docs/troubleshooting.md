# Troubleshooting

Find your problem below. Each fix starts with the quickest check. If nothing here helps, [ask for help](community.md#get-help), including what you tried and the output of the commands below.

## Starting the computer

### The USB stick won't boot

- **Written wrong.** Check the stick was *written* as an image (Fedora Media Writer, balenaEtcher or `dd`), not copied as a file.
- **Boot menu.** Choose the entry that starts with **UEFI** in the boot menu.
- **Secure Boot.** Ambar boots through Fedora's signed shim. If the firmware still says *security violation*, turn Secure Boot off in the firmware settings, start Ambar, and report your model.
- **USB port.** Some machines only boot from USB 2 ports or from the left-hand ports. Try another port.

### Black or garbled screen after the boot menu

Restart and choose **Try Ambar OS (safe graphics)**. This adds `nomodeset`. If that works, your graphics chip needs a driver fix. Please [send a hardware report](hardware.md#report-your-computer).

On an installed system, press **e** at the boot menu, add `nomodeset` to the end of the line that starts with `linux`, and press **Ctrl+X**.

### Something broke after an update

Every update keeps the previous version. Restart, and in the boot menu choose the **second** entry. That's the version you had before. To make that permanent:

```sh
sudo bootc rollback
sudo reboot
```

`sudo bootc status` shows which versions are installed. Your files and automations aren't affected by updates or rollbacks.

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

### The installer doesn't show my disk

Run `ambar-install --list`. It lists every disk it hid and why:

- it's the live USB itself
- it's read-only
- it's smaller than 32 GB
- it's mounted

Unmount a mounted disk (for example in Dolphin) and try again.

### The install stopped with an error

The installer downloads the OS image. Check the network, then run it again from a terminal to see the full output:

```sh
ambar-install --disk /dev/<disk>
```

## Collect information for a bug report

```sh
ambar --version
sudo bootc status
ambar service status
ambar history --steps | head -40
journalctl --user -u ambard -n 100 --no-pager
ambar-hwreport
```

Check the output before posting it, and remove anything you consider private, such as file names in your paths.
