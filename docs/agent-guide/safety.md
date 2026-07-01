# Safety & Guardrails

Fez protects against dangerous operations with a layered safety model. This
page covers the mutation workflow, guardrails, and when to use `--force`.

## Mutation Workflow

Every mutation should follow this pattern:

1. **Read current state.**
2. **Inspect the descriptor:** `fez describe <mutation-id> --json`.
3. **Dry-run when supported:** `--dry-run --json`.
4. **Execute** only when the target and requested change are clear.
5. **Verify** with a read-only command.

```bash
# Example: restarting a service
fez services status nginx.service --json          # 1. read
fez describe services.restart --json              # 2. inspect
fez services restart nginx.service --dry-run --json  # 3. dry-run
fez services restart nginx.service --json         # 4. execute
fez services status nginx.service --json          # 5. verify
```

## Protected Services

Operations on these services and sockets require `--force`:

`sshd.service`, `sshd.socket`, `ssh.service`, `ssh.socket`, `cockpit*`, `fez*`

Attempting to stop, restart, mask, or unmask a protected service without
`--force` returns exit 8 (`protected-unit`).

## Dangerous Package Transactions

Removing these packages or transactions with large cascading removals triggers
guardrails:

`kernel*`, `systemd*`, `glibc`, `dnf*`, `rpm*`, `sudo`, `openssh-server`,
`cockpit*`, `dbus*`

Blocked transactions return exit 10 (`dangerous-transaction`).

## Firewall Lockout Prevention

These firewall operations require `--force`:

- Removing the current SSH service or port
- Changing the default zone
- Enabling panic mode
- Reloading with drift between runtime and permanent config
- Disabling masquerade

## System Power Actions

`reboot`, `poweroff`, and `halt` require `--force`.

## When to Use `--force`

Only use `--force` when **all** of these are true:

- The user explicitly asked for the risky operation or confirmed it after
  seeing the risk.
- `fez describe <id> --json` shows the command and flags involved.
- A dry-run or read-only check confirms the target resource.
- You can explain which guardrail is being bypassed and why.

## Privilege Escalation

Privileged operations escalate through cockpit. Fez does not handle sudo
passwords. `access-denied` (exit 11) often means the target lacks a usable
cockpit escalation mechanism, or policy allows discovery but denies mutation.

Use `FEZ_ESCALATION=off` to disable escalation entirely. Any other value
forces that single mechanism with no fallback.
