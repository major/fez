# Fez Remote Host Reference

## Target a Remote Host

Use `--host` with a hostname or SSH config alias:

```bash
fez --host web1 system show --json
fez --host web1 services status nginx.service --json
fez --host web1 services restart nginx.service --dry-run --json
```

If the user names a remote host, include `--host`. Do not silently run the command against localhost.

## SSH Configuration

`fez` shells out to the system OpenSSH client. SSH config aliases work. Use `FEZ_SSH_CONFIG` to point fez at a specific SSH config file when the environment requires one:

```bash
FEZ_SSH_CONFIG=/path/to/ssh_config fez --host web1 system show --json
```

## Explicit Identities

By default, fez lets OpenSSH use its normal identity and agent behavior. Add `--ssh-identities-only` only when the user or environment requires OpenSSH `IdentitiesOnly=yes`:

```bash
fez --host web1 --ssh-identities-only system show --json
```

The environment variable form is:

```bash
FEZ_SSH_IDENTITIES_ONLY=1 fez --host web1 system show --json
```

## Target Requirements

The target needs `cockpit-bridge`. Remote transport needs `openssh-clients` locally. Some capabilities need target subsystem packages such as PCP, fwupd, RHSM, firewalld, UDisks2, systemd-resolved, dnf5daemon, or PackageKit. When a dependency is missing, prefer the JSON `hints` remediation over guessing.
