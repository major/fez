#!/bin/bash
set -eux

# fez drives four subsystems over cockpit-bridge:
#   services  -> systemd        (always present)
#   packages  -> dnf5daemon     (dnf5daemon-server; Fedora only, see below)
#   network   -> NetworkManager (always present)
#   firewall  -> firewalld      (firewalld)
# Install the bridge + cockpit-system (ships the sudo/pkexec superuser bridges
# cockpit-bridge alone lacks) plus the package/firewall backends so every
# capability hits a live service instead of the absent-service path.
#
# dnf5daemon-server (the org.rpm.dnf.v0 provider) exists on Fedora 41+ but NOT
# on RHEL 10, which is a dnf4 stack. Terraform sets with_dnf5daemon=false for
# RHEL so we never ask dnf to install a package that does not exist; under
# `set -e` that would abort cloud-init before the ready marker and kill the
# whole host. With it omitted, RHEL boots ready, services/network/firewall run,
# and the packages e2e test hits fez's exit-9 dependency-missing path and
# records `skip` (the harness expects this).
#
# This file is rendered by Terraform's templatefile(): a single-$ $${login_user}
# is substituted with the template variable's value, while a double-$$
# $${login_user} renders as the literal text. (Both tokens here are escaped as
# $$ so this comment is not itself interpolated.) The dnf/sudoers code below
# uses the single-$ form so Terraform fills in the values; there are no runtime
# shell $${VAR} expansions that need escaping.
#
# The install line uses a Terraform conditional rather than a runtime shell
# branch: when with_dnf5daemon is false (RHEL 10) the package is simply omitted
# from the argv, so dnf is never asked for a package that does not exist and
# `set -e` has nothing to trip over. This is cleaner than installing it
# best-effort and swallowing the error, because a genuine install failure on a
# host that SHOULD have it (Fedora) still aborts and fails the host.
dnf -y install cockpit-bridge cockpit-system firewalld${with_dnf5daemon ? " dnf5daemon-server" : ""}

# firewalld is not enabled by default on cloud images; the firewall capability
# needs the daemon running to exercise reads + runtime mutations.
systemctl enable --now firewalld

# On hosts that installed it (Fedora), dnf5daemon activates on demand over
# D-Bus; no explicit enable needed. Harmless no-op where it was not installed.
systemctl list-unit-files 'dnf5daemon*' >/dev/null 2>&1 || true

# cockpit-system's default superuser bridge escalates with `sudo -k -A`. A
# headless agent cannot answer an askpass prompt, so the login user needs
# passwordless sudo for transparent escalation to go through.
cat >/etc/sudoers.d/99-fez-e2e <<SUDO
${login_user} ALL=(ALL) NOPASSWD: ALL
SUDO
chmod 440 /etc/sudoers.d/99-fez-e2e

touch /var/lib/fez-e2e-ready   # marker the runner polls
