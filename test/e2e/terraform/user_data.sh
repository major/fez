#!/bin/bash
set -eux
# fez issues privileged systemd calls over cockpit-bridge's superuser channel
# ("superuser":"require"). cockpit-bridge alone ships NO superuser escalation
# bridges, so every privileged channel is denied with `access-denied`. The
# sudo/pkexec superuser bridge configs live in the cockpit-system package, so
# install it alongside the bridge.
dnf -y install cockpit-bridge cockpit-system

# cockpit-system's default superuser bridge escalates with `sudo -k -A`. A
# headless agent cannot answer an askpass prompt, so the invoking user needs
# passwordless sudo for the escalation to go through without interaction.
cat >/etc/sudoers.d/99-fez-e2e <<'SUDO'
fedora ALL=(ALL) NOPASSWD: ALL
SUDO
chmod 440 /etc/sudoers.d/99-fez-e2e

touch /var/lib/fez-e2e-ready   # marker the runner polls
