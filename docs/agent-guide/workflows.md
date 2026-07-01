# Workflows

End-to-end examples for common agent tasks. Every workflow starts with
orientation and discovery, then follows the safe mutation pattern.

## Start Every Session

```bash
fez guide --json
fez system show --json
fez capabilities
```

## Inspect a Host

```bash
fez system show --json
fez system metrics --json
fez services list --state failed --json
fez storage health --json
fez dns status --json
```

## Triage a Failing Service

```bash
# 1. Check status
fez services status nginx.service --json

# 2. Check recent logs
fez services logs nginx.service --lines 100 --json

# 3. If restart is needed, follow the safe pattern
fez describe services.restart --json
fez services restart nginx.service --dry-run --json
fez services restart nginx.service --json
fez services status nginx.service --json
```

## Install a Package

```bash
# 1. Search
fez packages search nginx --json

# 2. Get details
fez packages info nginx --json

# 3. Check for updates first
fez packages check-update --json

# 4. Safe install
fez describe packages.install --json
fez packages install nginx --dry-run --json
fez packages install nginx --json
```

On failure, read the JSON envelope: `error.code` and `hints` tell you what
went wrong and how to fix it. Do not guess package names — `hints` has
target-specific remediation.

## Check and Adjust the Firewall

```bash
# 1. Read current state
fez firewall status --json
fez firewall list --json

# 2. Add a service
fez describe firewall.add-service --json
fez firewall add-service public https --dry-run --json
fez firewall add-service public https --json

# 3. Persist runtime changes
fez firewall confirm --json
```

Firewall changes are runtime-only by default. Use `fez firewall confirm` to
persist them permanently. Guardrails protect against lockout-prone operations
(removing SSH, changing default zone, enabling panic mode).

## Make Any Safe Change

The pattern is the same for every mutation:

1. Confirm the target host and resource.
2. Capture current state with a read-only command.
3. Run `fez describe <mutation-id> --json`.
4. Dry-run when the command supports it.
5. Execute only after the plan matches user intent.
6. Verify with a read-only command.
