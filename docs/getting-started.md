# Getting Started

## Install

```bash
sudo dnf install fez
```

Requires `cockpit-bridge` and `openssh-clients`; both are pulled in automatically.

Minimum target: Fedora 41+ or RHEL 10+ (for dnf5daemon). Older releases fall
back to PackageKit.

## Your first command

```bash
fez system show --json
```

This prints a machine-readable overview of the host: OS release, hostname,
uptime, kernel version, and available subsystems. The `--json` flag gives you
the compact `fez/v1` envelope every command supports.

## Understanding the output

Every `--json` response uses the same envelope:

```json
{"apiVersion":"fez/v1","kind":"SystemOverview","host":"localhost","status":"ok","data":{...}}
```

- **`status`** — `"ok"` or `"error"`. Check this first.
- **`data`** — the payload when successful.
- **`error`** — structured error with `code`, `message`, and optional `detail`.
- **`hints`** — actionable remediation (install a missing package, add `--force`, etc.).

Without `--json`, fez prints human-readable output. Agents should always use `--json`.

## Discovering what fez can do

```bash
# List all available capabilities
fez capabilities

# Inspect a specific command before using it
fez describe services.status --json
fez describe packages.install --json
```

Descriptors show inputs, flags, output kind, privilege requirements, examples,
and JSON schema. Run `fez describe <id> --json` before any unfamiliar command.

## Targeting a remote host

```bash
fez --host web1 system show --json
fez --host web1 services status nginx.service --json
```

`--host` accepts hostnames and SSH config aliases. fez shells out to the system
OpenSSH client. The target needs `cockpit-bridge` installed.

## Making changes safely

Fez has a safety layer that protects critical services, dangerous package
removals, and lockout-prone firewall changes. The safe workflow is:

```bash
# 1. Check current state
fez services status nginx.service --json

# 2. Inspect the command
fez describe services.restart --json

# 3. Dry-run when supported
fez services restart nginx.service --dry-run --json

# 4. Execute
fez services restart nginx.service --json

# 5. Verify
fez services status nginx.service --json
```

If a guardrail blocks an operation, fez tells you why and suggests `--force`.
Only use `--force` when you understand the risk and the user has confirmed.

## Next steps

- [Agent Guide](agent-guide/index.md) — the full operating loop for LLM agents
- [Discovery & Describe](agent-guide/discovery.md) — `fez capabilities`, `fez describe`, `fez guide`
- [Capability Reference](reference/index.md) — every command fez can run
- [JSON Envelope](agent-guide/json-envelope.md) — deep dive on parsing responses
- [Safety & Guardrails](agent-guide/safety.md) — when and how to use `--force`
