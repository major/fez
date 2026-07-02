# Firewall

Manage the firewalld firewall: inspect zones and services, add or remove rules,
and persist runtime changes.

**Backend:** firewalld over D-Bus. Requires `firewalld` on the target.
When firewalld is absent or unreachable, returns exit 9 (`dependency-missing`).

## Read

### `firewall status`

Show firewall state, the default zone, panic mode flag, and any uncommitted
runtime-vs-permanent drift.

```
fez firewall status --json
```

Output kind: `FirewallStatus`. The `pending_changes` field lists differences
between the runtime and permanent config — services, ports, or settings that
exist at runtime but have not been persisted.

`status` reads permanent config to compute drift. If the target has no
escalation mechanism, `status` may fail with exit 11 even though it's mostly
read-only.

### `firewall list`

List all zones with a per-zone summary.

```
fez firewall list --json
```

Output kind: `FirewallZoneList`. Each zone entry shows whether it's the
default zone, its services, ports, interfaces, and sources.

### `firewall show`

Show one zone's full detail.

```
fez firewall show public --json
```

Output kind: `FirewallZone`. Unknown zones return exit 4 (`not-found`).

### `firewall services`

List the service catalog firewalld knows about.

```
fez firewall services --json
```

Output kind: `FirewallServiceCatalog`. These are the valid arguments to
`add-service` — predefined service definitions like `http`, `https`, `ssh`,
`cockpit`, etc.

## Write

All firewall mutations are **privileged** (require cockpit escalation) and
audit-logged. Mutations apply to the **runtime config only**. They are not
permanent until `fez firewall confirm`.

### Runtime + Confirm Model

```bash
fez firewall add-service public https --json   # runtime only
fez firewall status --json                      # check for pending_changes
fez firewall confirm --json                     # persist to permanent
```

Every mutation command changes the runtime config. `fez firewall confirm` calls
firewalld's `runtimeToPermanent` to commit all pending changes at once. Use
`fez firewall status` to review pending changes before confirming.

### `firewall add-service`

Add a service to a zone (runtime only).

```
fez firewall add-service http --json
fez firewall add-service http --zone public --timeout 60
```

| Flag | Purpose |
|------|---------|
| `--zone <z>` | Target zone (defaults to the host's default zone) |
| `--timeout <n>` | Auto-revert after N seconds |

### `firewall remove-service`

Remove a service from a zone (runtime only).

```
fez firewall remove-service http --json
fez firewall remove-service http --zone public
```

Removing the SSH service (which carries the active session) is refused without
`--force` (exit 8).

### `firewall add-port`

Add a port to a zone (runtime only).

```
fez firewall add-port 8080/tcp --json
fez firewall add-port 8080/tcp --zone public --timeout 300
```

Ports are specified as `port/proto` (e.g. `8080/tcp`, `53/udp`).

### `firewall remove-port`

Remove a port from a zone (runtime only).

```
fez firewall remove-port 8080/tcp --json
```

Removing the port that carries the active SSH session is refused without
`--force` (exit 8).

### `firewall set-default-zone`

Set the default firewall zone. Every default-zone change requires `--force`
(exit 8 without it), because a different default can sever connections.

```
fez firewall set-default-zone internal --force --json
```

### `firewall reload`

Reload the permanent config into runtime, discarding any uncommitted runtime
changes. When uncommitted drift exists, reload is refused without `--force`
since it would lose that work. With no drift, it runs freely.

```
fez firewall reload --json
```

### `firewall confirm`

Persist the current runtime config to permanent. This is the only persistence
path — all mutations are runtime-only until confirmed.

```
fez firewall confirm --json
```

### `firewall panic`

Toggle panic mode. `panic on` drops **all** traffic and is refused without
`--force` (exit 8). `panic off` re-enables normal traffic rules.

```
fez firewall panic on --force
fez firewall panic off --json
```

### `firewall masquerade`

Enable or disable masquerade (SNAT) for a zone. Enabling is unguarded;
disabling is refused without `--force` because dropping SNAT can sever
forwarded clients.

```
fez firewall masquerade on --json
fez firewall masquerade off --zone public --force
```

| Flag | Purpose |
|------|---------|
| `--zone <z>` | Target zone (defaults to the host's default zone) |
| `--timeout <n>` | Auto-revert after N seconds (ignored for `off`) |

## Firewall Guardrails

These operations require `--force`:

- Removing the SSH service or port
- Changing the default zone
- Enabling panic mode
- Reloading with uncommitted drift
- Disabling masquerade

All return exit 8 (`protected-unit`) when refused.

## Unsupported APIs

Older firewalld versions may lack `getMasquerade`. Fez maps `UnknownMethod`
errors to exit 12 (`unsupported-api`) rather than `dependency-missing`.
