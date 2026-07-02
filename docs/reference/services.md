# Services

Manage systemd services: list units, inspect status, read logs, and control
lifecycle (start, stop, restart, reload, enable, disable).

**Backend:** systemd over cockpit D-Bus. No extra target packages required.

## Read

### `services list`

List systemd units on the target host.

```
fez services list --state failed --json
```

| Flag | Purpose |
|------|---------|
| `--state <s>` | Filter by active state: `active`, `inactive`, `activating`, `deactivating`, `failed`, `maintenance`, `reloading` |

Output kind: `ServiceList`. Each entry includes the unit name, load state,
active state, sub-state, and description.

### `services status`

Show the full status of a single unit.

```
fez services status sshd.service --json
```

Output kind: `ServiceStatus`. Includes active state, sub-state, enablement
(enabled/disabled/static), the unit file path, and the preset policy.

### `services logs`

Read journal entries for a unit.

```
fez services logs nginx.service --lines 100 --since '1 hour ago' --json
```

| Flag | Purpose |
|------|---------|
| `--since <expr>` | Journalctl time expression (`'1 hour ago'`, `'2026-07-01'`) |
| `--priority <p>` | Minimum priority: `emerg`, `alert`, `crit`, `err`, `warning`, `notice`, `info`, `debug`, or `0`-`7` |
| `--lines <n>` | Limit to the last N entries |
| `--follow` | Stream new entries as they arrive |

Output kind: `LogEntries`. With `--follow`, output is streamed directly to
stdout and the envelope is skipped.

## Write

All service mutations are **privileged** (require cockpit escalation) and audit-logged.
Protected units (SSH, cockpit, fez) are refused without `--force` (exit 8).

### `services start`

Start a unit immediately.

```
fez services start nginx.service --json
```

### `services stop`

Stop a running unit.

```
fez services stop nginx.service --json
```

### `services restart`

Restart a unit (stop then start).

```
fez services restart nginx.service --json
```

### `services reload`

Reload a unit's configuration without a full restart. Only works for services
that support reload.

```
fez services reload nginx.service --json
```

### `services enable`

Enable a unit to start at boot. Add `--now` to also start it immediately.

```
fez services enable chronyd.service --json
fez services enable chronyd.service --now
```

### `services disable`

Disable a unit from starting at boot. Add `--now` to also stop it immediately.

```
fez services disable chronyd.service --json
fez services disable chronyd.service --now
```

## Protected Units

These units are refused for stop, restart, enable, and disable without `--force`:

`sshd.service`, `sshd.socket`, `ssh.service`, `ssh.socket`, `cockpit*`, `fez*`

Attempting a protected operation without `--force` returns exit 8
(`protected-unit`). The error hints field tells you which guardrail blocked it.
