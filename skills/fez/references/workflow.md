# Fez Workflow Reference

## Start Every Investigation

```bash
fez guide --json
fez system show --json
fez capabilities
```

Use `fez guide --json` to refresh the CLI contract. Use `fez system show --json` to orient on hostname, OS, kernel, time, and hardware. Use `fez capabilities` before choosing a command family.

## Use Descriptors Before Unfamiliar Commands

```bash
fez describe system.metrics --json
fez describe services.restart --json
fez describe packages.install --json
```

Descriptors tell you inputs, flags, output kind, examples, privilege requirements, and schemas. Prefer descriptors over memory when a command is unfamiliar.

## Inspect a Host

```bash
fez system show --json
fez system metrics --json
fez services list --state failed --json
fez storage health --json
fez dns status --json
```

Report concise findings from `data`. If a command returns `status:"error"`, read `error.code`, `error.message`, and `hints` before choosing the next command.

## Triage a Service

```bash
fez services status nginx.service --json
fez services logs nginx.service --lines 100 --json
fez describe services.restart --json
fez services restart nginx.service --dry-run --json
fez services restart nginx.service --json
fez services status nginx.service --json
```

Check status and logs before mutation. Dry-run before restart when the user asked for a change and the command supports it. Verify status after mutation.

## Investigate Packages

```bash
fez packages search nginx --json
fez packages info nginx --json
fez packages check-update --json
fez describe packages.install --json
```

For failures, use the JSON envelope. `dependency-missing` usually means a backend such as dnf5daemon or PackageKit is absent on the target. Use `hints` for remediation text.

## Make a Safe Change

1. Confirm target host and resource.
2. Run a read-only command to capture current state.
3. Run `fez describe <mutation-id> --json`.
4. Run the mutation with `--dry-run` when meaningful.
5. Run the mutation without `--dry-run` only after the plan matches user intent.
6. Verify with a read-only command.
