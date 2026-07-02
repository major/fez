# Packages

Manage RPM packages: list, search, inspect, and mutate (install, remove, upgrade).

**Backend:** dnf5daemon (`org.rpm.dnf.v0`) is the primary backend. When dnf5daemon
is absent (RHEL 10, older Fedora), fez falls back to PackageKit automatically.
There is no `--backend` flag; selection is transparent.

## Read

### `packages list`

List installed (default) or available packages.

```
fez packages list --json
fez packages list --available --name nginx --limit 20
fez packages list --available --repo fedora --json
```

| Flag | Purpose |
|------|---------|
| `--available` | List available packages instead of installed |
| `--repo <id>` | Restrict to exact repository id (repeatable; OR union) |
| `--name <substr>` | Keep only packages whose name contains the substring |
| `--limit <n>` | Maximum rows to return |
| `--offset <n>` | Rows to skip before returning (default 0) |

Output kind: `PackageList`. JSON output includes pagination metadata:

```json
{"total": 4500, "returned": 20, "limit": 20, "offset": 0, "next_offset": 20}
```

Unpaginated responses over 1000 rows include a hint recommending `packages search`,
`--name`, or `--limit`. The applied repo filter is echoed in the envelope's `repos` field.

### `packages info`

Show the full attributes of a single package.

```
fez packages info bash --json
```

Output kind: `PackageInfo`. Includes version, release, arch, repo, size, summary,
description, and URL.

### `packages search`

Search available packages by name, summary, or provides.

```
fez packages search nginx --json
```

Output kind: `PackageSearch`. Each result includes the package name, summary,
and the matched field.

### `packages check-update`

List packages with available upgrades.

```
fez packages check-update --json
```

Output kind: `PackageUpdates`. Shows the installed version, available version,
and repository for each upgradable package.

### `packages repolist`

List repositories and their enabled state.

```
fez packages repolist --all --json
```

| Flag | Purpose |
|------|---------|
| `--enabled` | Show only enabled repositories (default) |
| `--disabled` | Show only disabled repositories |
| `--all` | Show all repositories |

Output kind: `RepoList`.

## Write

All package mutations are **privileged** and audit-logged. Removal guardrails
protect critical packages and large cascading transactions (exit 10).

### `packages install`

Install one or more packages. Resolves the transaction first and surfaces the
plan; `--dry-run` stops after the plan.

```
fez packages install htop --json
fez packages install nginx --dry-run
```

Output kind: `PackageMutation`. The plan shows packages to install, upgrade,
reinstall, downgrade, and remove. On the PackageKit backend, size fields are
`null` and the envelope carries a `"backend":"packagekit"` label with a
degraded-schema hint.

### `packages remove`

Remove one or more packages. Resolves first and applies removal guardrails.

```
fez packages remove oldpkg --json
fez packages remove oldpkg --dry-run
```

### `packages upgrade`

Upgrade named packages, or all packages when no spec is given.

```
fez packages upgrade --json
fez packages upgrade nginx --dry-run
```

## Backend Differences

| Feature | dnf5daemon | PackageKit |
|---------|:----------:|:----------:|
| Install/remove sizes | Yes | `null` |
| Repo filtering (server-side) | No (client-side) | No (client-side) |
| Signal-driven listing | No (direct calls) | Yes (collects signals) |
| RHEL availability | RHEL 11+ | RHEL 10 |

When both backends are absent, `packages` commands return exit 9
(`dependency-missing`) with remediation naming both daemons.

## Removal Guardrails

These packages and patterns are protected from removal without `--force`:

`kernel*`, `systemd*`, `glibc`, `dnf*`, `rpm*`, `sudo`, `openssh-server`,
`cockpit*`, `dbus*`

Transactions that would remove more packages than a safety threshold are also
blocked. Both return exit 10 (`dangerous-transaction`).
