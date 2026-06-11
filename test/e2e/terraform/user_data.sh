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
# $${login_user} renders as the literal text. (Tokens that should reach the
# guest as literal shell are escaped as $$.) The interpolated values used below
# are ${login_user} and ${os_family}; runtime shell $$VAR expansions are escaped
# so Terraform does not try to resolve them.

# Always-present surface: bridge, superuser bridges, and firewalld. These exist
# on every target, so a failure here is a real provisioning fault and SHOULD
# abort (set -e) and fail the host.
dnf -y install cockpit-bridge cockpit-system firewalld

# packages backend (dnf5daemon-server) is OPTIONAL and OS-dependent:
#   - Fedora 41+ ships it; we hard-require it so a silent install regression
#     surfaces as a real failure (host never readies -> infra fail), not a
#     false "skip" that would hide a broken packages capability.
#   - RHEL 10 does NOT ship it: RHEL 10 uses dnf4 as the system manager and
#     dnf5/dnf5daemon target RHEL 11 (upstream dnf5 PR #780 explicitly blocks
#     dnf5 from replacing dnf on RHEL 10). The package is absent from
#     BaseOS/AppStream, so installing it best-effort lets the host still ready
#     and run services/network/firewall; `fez packages` then returns exit 9
#     (dependency-missing) and the capability harness records "skip", not
#     "fail" (issue #50).
if [ "${os_family}" = "rhel" ]; then
  dnf -y install dnf5daemon-server || \
    echo "fez-e2e: dnf5daemon-server unavailable on ${os_family}; packages capability will be skipped" >&2
else
  dnf -y install dnf5daemon-server
fi

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
