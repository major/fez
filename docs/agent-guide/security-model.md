# Security Model

Fez is a stateless management CLI for Fedora and RHEL systems. It does not run a
long-lived agent, keep local inventory, cache host state, or store credentials.
Each invocation connects to the target host, asks live system services for state,
and exits.

The security model is intentionally simple:

- **Transport authentication** is handled by OpenSSH for remote hosts.
- **Host management access** is handled by `cockpit-bridge`.
- **Privilege escalation** is handled by the target host's cockpit, sudo, and
  polkit configuration.
- **Fez guardrails** reduce accidental high-impact changes, but do not replace
  operating system authorization policy.

## Trust Boundaries

```text
Operator workstation
  - fez CLI
  - OpenSSH client
  - user SSH identity
        |
        | SSH transport for remote hosts
        v
Managed Fedora/RHEL host
  - cockpit-bridge
  - system D-Bus services
  - sudo / polkit policy
        |
        | host authorization boundary
        v
Host subsystems own state
  - systemd, firewalld, dnf
  - PackageKit, NetworkManager, ...

fez local process boundary
  - no fez daemon
  - no stored host state
  - no stored passwords
```

Fez is a client. The managed host remains the source of truth for users,
permissions, service state, package state, firewall state, and audit policy.

## Local Command Flow

For local commands, fez starts `cockpit-bridge` directly and speaks cockpit's
framed JSON protocol.

```text
fez command
   |
   v
spawn cockpit-bridge as the current user
   |
   v
open unprivileged cockpit channels
   |
   +--> read-only D-Bus calls when possible
   |
   +--> privileged cockpit channel only for admin-level operations
```

Read-only commands should remain unprivileged unless the underlying subsystem
itself requires elevated access for the required read.

## Remote Command Flow

For remote commands, fez shells out to the system OpenSSH client. SSH behavior is
therefore governed by normal OpenSSH configuration, host key checking, SSH agent
policy, certificates, bastions, and enterprise access controls.

```text
fez --host web1 ...
   |
   v
system ssh client
   |
   v
SSH session to web1
   |
   v
cockpit-bridge on web1
   |
   v
D-Bus services and host subsystems on web1
```

The target host must have `cockpit-bridge` installed. Some capabilities require
additional backend services such as firewalld, dnf5daemon, PackageKit,
NetworkManager, UDisks2, fwupd, PCP, or subscription-manager.

## Privilege Escalation for Admin Commands

Fez does not implement its own privilege system. Admin-level commands use
cockpit's privilege escalation path on the target host.

```text
admin-level fez command
   |
   v
connect to cockpit-bridge as the SSH/local user
   |
   v
ask cockpit which safe superuser mechanisms are available
   |
   v
start an advertised mechanism, such as sudo or polkit
   |
   v
open a privileged cockpit channel
   |
   v
call the target subsystem as root / authorized administrator
```

Important properties:

- Fez **does not prompt for sudo passwords**.
- Fez **does not store sudo passwords, SSH passwords, tokens, or host state**.
- Escalation requires host-side policy that cockpit can use, such as
  `cockpit-system` plus passwordless sudo or a suitable polkit rule.
- If escalation is unavailable or denied, fez returns `access-denied` with exit
  code 11.
- Read-only commands use unprivileged channels where possible.
- Mutations that require administrator rights open privileged channels only for
  the operation being performed.

You can disable escalation entirely:

```bash
FEZ_ESCALATION=off fez services restart nginx.service --json
```

Any other non-empty `FEZ_ESCALATION` value asks fez to use only that advertised
safe mechanism, with no fallback to another mechanism.

## Guardrails Are Not Authorization

Fez has a safety layer for operations that are easy to get wrong, such as:

- restarting or disabling SSH, cockpit, or fez-related services
- removing critical packages such as `sudo`, `systemd`, `rpm`, or `dnf`
- firewall changes that could lock out the current session
- power actions such as reboot and poweroff

These guardrails may require `--force`, but `--force` only bypasses fez's client
side safety check. It does **not** grant operating system privileges and does not
bypass sudo, polkit, cockpit, SSH, or subsystem authorization.

```text
requested mutation
   |
   v
fez guardrail check ---- requires --force? ---- yes/no
   |
   v
host authorization ---- sudo/polkit/cockpit policy ---- allow/deny
   |
   v
subsystem applies change, or fez returns a structured error
```

Use `--force` only after you have confirmed the target resource and can explain
which guardrail you are bypassing.

## Audit Logging

Mutating commands are wrapped in fez's audit flow. A mutation writes an
`attempt` record before the operation and then writes an `ok` or `error` result
record after the operation finishes.

```text
mutation requested
   |
   v
write audit attempt
   |
   v
perform operation
   |
   v
write audit result: ok or error
```

Audit writes are best effort: audit failure should not be able to break the
managed operation. Fez itself does not read audit records back as state.

## Enterprise Deployment Notes

For enterprise environments:

- Manage remote access with standard OpenSSH controls: host keys, SSH
  certificates, bastion hosts, allowed users, and SSH config policy.
- Manage administrator authorization on the target host with sudo and polkit.
- Install `cockpit-system` where privileged cockpit escalation is required.
- Avoid giving broad passwordless sudo access to users or automation that do not
  need mutation capabilities.
- Use `FEZ_ESCALATION=off` for discovery-only workflows where escalation must be
  impossible.
- Prefer `fez describe <capability> --json` before granting automation access;
  descriptors show whether a capability is privileged.
- Monitor the configured audit destination for mutation attempts and results.
- Install only the backend services needed for the capabilities you plan to use.

## Common Security-Related Failures

| Symptom | Likely cause | What to check |
| --- | --- | --- |
| `access-denied` / exit 11 | Escalation failed or policy denied the operation | `cockpit-system`, sudoers, polkit rules, user group membership |
| Privileged commands fail, read-only commands work | User can connect but cannot become an administrator through cockpit | cockpit superuser mechanisms and sudo/polkit policy |
| `dependency-missing` / exit 9 | Required host backend is absent | JSON `hints` field for the package or service to install |
| `protected-unit` / exit 8 | Fez blocked a risky service or power operation | Confirm the target and use `--force` only when intentional |
| `dangerous-transaction` / exit 10 | Package change could remove critical packages or a large cascade | Review the transaction and avoid broad removals |

For tactical guardrails and `--force` guidance, see
[Safety & Guardrails](safety.md). For SSH targeting details, see
[Remote Hosts](remote-hosts.md).
