# Fez Workflow Reference

## Start Every Investigation

```bash
fez guide --json
fez system show --json
fez capabilities
```


## Use Descriptors Before Unfamiliar Commands

```bash
fez describe system.metrics --json
fez describe services.restart --json
fez describe packages.install --json
```

Descriptors show inputs, flags, output kind, privilege, examples, and schema.

## Inspect a Host

```bash
fez system show --json
fez system metrics --json
fez services list --state failed --json
fez storage health --json
fez dns status --json
```


## Triage a Service

```bash
fez services status nginx.service --json
fez services logs nginx.service --lines 100 --json
fez describe services.restart --json
fez services restart nginx.service --dry-run --json
fez services restart nginx.service --json
fez services status nginx.service --json
```


## Investigate Packages

```bash
fez packages search nginx --json
fez packages info nginx --json
fez packages check-update --json
fez describe packages.install --json
```

On failure, read the JSON envelope and use `hints` for remediation.

## Make a Safe Change

1. Confirm target host and resource.
2. Capture current state.
3. Run `fez describe <mutation-id> --json`.
4. Dry-run when meaningful.
5. Execute only after the plan matches user intent.
6. Verify with a read-only command.
