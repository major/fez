# Capability Reference

Every command fez can run. For machine-readable detail, use `fez describe <id> --json`.

## Subsystem Map

| Subsystem | Backend | Read | Write |
|-----------|---------|:----:|:-----:|
| [services](services.md) | systemd over cockpit D-Bus | list, status, logs | start, stop, restart, reload, enable, disable |
| [packages](packages.md) | dnf5daemon (preferred), PackageKit (fallback) | list, info, search, check-update, repolist | install, remove, upgrade |
| [network](network.md) | NetworkManager | list, show | — |
| [firewall](firewall.md) | firewalld | status, list, show, services | add/remove-service, add/remove-port, set-default-zone, reload, confirm, panic, masquerade |
| [system](system.md) | hostname1, timedate1, logind, RHSM, fwupd, PCP | show, metrics, sessions, users, inhibitors, boot-entries, subscription, firmware | reboot, poweroff, suspend |
| [storage](storage.md) | UDisks2 | list, show, health | — |
| [dns](dns.md) | systemd-resolved (preferred), NetworkManager DnsManager (fallback) | status, query | flush |
| [journal](journal.md) | journalctl via cockpit stream | query, list-boots, list-fields | — |

## Global Flags

Every command accepts these flags anywhere on the command line:

| Flag | Purpose |
|------|---------|
| `--host <h>` | Target host (defaults to localhost) |
| `--json` | Emit a `fez/v1` JSON envelope |
| `--dry-run` | Preview mutations without applying (no-op for reads) — [see dry-run page](dry-run.md) |
| `--force` | Bypass safety guardrails |
| `--ssh-identities-only` | Restrict SSH to configured identities |

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `FEZ_BRIDGE` | Override the cockpit-bridge binary path |
| `FEZ_AUDIT` | Path to the audit log sink (JSON-lines) |
| `FEZ_SSH_CONFIG` | Path to an SSH config file (defaults to `~/.ssh/config`) |
| `FEZ_SSH_IDENTITIES_ONLY` | If set to `1`, equivalent to `--ssh-identities-only` |
| `FEZ_ESCALATION` | Disable (`off`) or force a specific escalation mechanism |
| `FEZ_ACTOR` | Actor identity recorded in audit entries |
| `FEZ_CORRELATION_ID` | Correlation id recorded in audit entries |
| `FEZ_TARGET_HOST` | Override the target host label in audit records |
| `FEZ_OPERATION` | Operation label recorded in audit entries |
| `FEZ_UNIT` | Unit label recorded in audit entries |

The audit env vars (`FEZ_ACTOR`, `FEZ_CORRELATION_ID`, etc.) are metadata
attached to audit log records. They do not affect command behavior.

## Packaging Commands

`fez man` emits a roff man page on stdout. It is hidden from `fez --help`
because it is only used by packaging scripts (e.g. RPM `%build`).

## Target Dependencies

Some capabilities need extra packages on the target host. When a dependency is
missing, the JSON `hints` field names the exact package.

| Capability | Target dependency |
|-----------|-------------------|
| `system metrics` | `pcp`, `python3-pcp` |
| `system firmware` | `fwupd` |
| `system subscription` | `subscription-manager` (RHEL only) |
| `firewall` (any) | `firewalld` |
| `storage` (any) | `udisks2` |
| `dns` (any) | `systemd-resolved` (preferred) or NetworkManager |
| `packages` (any) | `dnf5daemon-server` (preferred) or PackageKit |
