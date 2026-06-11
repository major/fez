#!/bin/bash
set -eux

# fez drives four subsystems over cockpit-bridge:
#   services  -> systemd        (always present)
#   packages  -> dnf5daemon     (dnf5daemon-server)
#   network   -> NetworkManager (always present)
#   firewall  -> firewalld      (firewalld)
# Install the bridge + cockpit-system (ships the sudo/pkexec superuser bridges
# cockpit-bridge alone lacks) plus the package/firewall backends so every
# capability hits a live service instead of the absent-service path.
#
# This file is rendered by Terraform's templatefile(); $${...} escapes a literal
# shell brace, $${login_user} is substituted by Terraform (NetworkManager etc.
# need no substitution and use plain shell).
dnf -y install cockpit-bridge cockpit-system dnf5daemon-server firewalld

# firewalld is not enabled by default on cloud images; the firewall capability
# needs the daemon running to exercise reads + runtime mutations.
systemctl enable --now firewalld

# dnf5daemon activates on demand over D-Bus; no explicit enable needed, but make
# sure the unit is present so packages tests do not hit dependency-missing.
systemctl list-unit-files 'dnf5daemon*' >/dev/null 2>&1 || true

# cockpit-system's default superuser bridge escalates with `sudo -k -A`. A
# headless agent cannot answer an askpass prompt, so the login user needs
# passwordless sudo for transparent escalation to go through.
cat >/etc/sudoers.d/99-fez-e2e <<SUDO
${login_user} ALL=(ALL) NOPASSWD: ALL
SUDO
chmod 440 /etc/sudoers.d/99-fez-e2e

touch /var/lib/fez-e2e-ready   # marker the runner polls
