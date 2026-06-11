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
# This file is rendered by Terraform's templatefile(): a single-$ $${login_user}
# is substituted with the template variable's value, while a double-$$
# $${login_user} renders as the literal text. (Both tokens here are escaped as
# $$ so this comment is not itself interpolated.) The dnf/sudoers code below
# uses the single-$ form so Terraform fills in the login user; there are no
# runtime shell $${VAR} expansions that need escaping.
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
