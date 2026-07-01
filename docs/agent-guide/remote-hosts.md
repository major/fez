# Remote Hosts

Fez reaches remote hosts by shelling out to the system OpenSSH client. This
page covers targeting, SSH configuration, and target requirements.

## Targeting a Remote Host

Use `--host` with a hostname or SSH config alias:

```bash
fez --host web1 system show --json
fez --host web1 services status nginx.service --json
fez --host web1 services restart nginx.service --dry-run --json
```

If the user names a remote host, include `--host`. Do not silently run the
command against localhost. Without `--host`, fez operates on the local machine.

## SSH Configuration

Fez uses the system OpenSSH client (`ssh`). SSH config aliases defined in
`~/.ssh/config` work automatically. Use `FEZ_SSH_CONFIG` to point fez at a
specific config file:

```bash
FEZ_SSH_CONFIG=/path/to/ssh_config fez --host web1 system show --json
```

## Explicit Identities

By default, fez lets OpenSSH use its normal identity and agent behavior
(`IdentitiesOnly=no`). Add `--ssh-identities-only` only when the user or
environment requires `IdentitiesOnly=yes`:

```bash
fez --host web1 --ssh-identities-only system show --json
```

The equivalent environment variable:

```bash
FEZ_SSH_IDENTITIES_ONLY=1 fez --host web1 system show --json
```

## Target Requirements

The remote host needs `cockpit-bridge` installed. Some capabilities need
additional target packages:

| Capability | Target dependency |
| --- | --- |
| `fez system metrics` | `pcp` (Performance Co-Pilot) |
| `fez system firmware` | `fwupd` |
| `fez system subscription` | `subscription-manager` (RHEL) |
| `fez firewall` | `firewalld` |
| `fez storage` | `udisks2` |
| `fez dns status` | `systemd-resolved` (preferred) or NetworkManager |
| `fez packages` | `dnf5daemon` (preferred) or PackageKit |

When a dependency is missing, the JSON `hints` field gives the specific
package to install. Prefer hints over guessing.
