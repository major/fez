# Dry-Run Semantics

`--dry-run` is a global flag. On mutations, it resolves the operation without
applying it and returns a preview of what would happen. On read-only commands,
it is silently ignored.

## Which Commands Support It

| Command | What `--dry-run` returns |
|---------|---------------------------|
| `services start` | `kind: "DryRun"` — the operation, unit, target host, and the full CLI invocation that would execute |
| `services stop` | `kind: "DryRun"` |
| `services restart` | `kind: "DryRun"` |
| `services reload` | `kind: "DryRun"` |
| `services enable` | `kind: "DryRun"` |
| `services disable` | `kind: "DryRun"` |
| `packages install` | `kind: "PackagePlan"` — the resolved transaction plan with install/remove/upgrade/downgrade arrays, counts, and total install size |
| `packages remove` | `kind: "PackagePlan"` |
| `packages upgrade` | `kind: "PackagePlan"` |

Commands that accept `--dry-run` advertise it in their `fez describe` output
under `flags`. Commands that don't (reads, firewall mutations, power actions)
silently accept the flag without effect.

## Dry-Run Output Shape

### Service Mutations (`DryRun`)

```json
{
  "kind": "DryRun",
  "data": {
    "operation": "start",
    "unit": "nginx.service",
    "host": "localhost",
    "privileged": true,
    "command": "fez --host localhost services start nginx.service --json"
  }
}
```

The `command` field is the exact CLI invocation fez would execute — useful for
logging before a real run.

### Package Mutations (`PackagePlan`)

```json
{
  "kind": "PackagePlan",
  "data": {
    "operation": "install",
    "specs": ["nginx"],
    "dry_run": true,
    "install": [
      {"name": "nginx", "evr": "1:1.24.0-1.fc44", "arch": "x86_64", "repo_id": "fedora", "install_size": 1200000}
    ],
    "remove": [],
    "upgrade": [],
    "downgrade": [],
    "install_size_total": 1200000,
    "counts": {"install": 1, "remove": 0, "upgrade": 0, "downgrade": 0},
    "backend": "dnf5daemon"
  }
}
```

The plan shows every package that would be:

- `install` — newly installed
- `remove` — removed (due to conflicts or obsoletes)
- `upgrade` — upgraded as a dependency
- `downgrade` — downgraded to satisfy constraints

`counts` summarizes each category. `install_size_total` is the aggregate
download/install size in bytes (`null` on the PackageKit backend).

The plan is identical in structure to a real `PackageMutation` response — the
`dry_run` boolean is the only difference. This means you can send the same
plan-parsing code for both preview and execution.

## Removal Guardrails During Dry-Run

Package `--dry-run` still applies removal guardrails. If the resolved plan would
remove a protected package or exceed the cascade limit, `--dry-run` returns the
same exit 10 (`dangerous-transaction`) error. Use `--force` to preview the plan
anyway.

## Safety Pattern

```bash
# 1. Inspect the command
fez describe packages.install --json

# 2. Preview the plan
fez packages install nginx --dry-run --json

# 3. Review counts, removed packages, and size
#    (parse PackagePlan.counts and PackagePlan.remove)

# 4. Execute
fez packages install nginx --json
```
