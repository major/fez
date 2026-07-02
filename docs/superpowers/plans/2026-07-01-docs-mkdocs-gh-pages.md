# fez Docs Site — mkdocs + GitHub Pages Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Publish narrative docs for fez on GitHub Pages using mkdocs+material, built and deployed via GitHub Actions.

**Architecture:** mkdocs reads markdown from `docs/` and renders with material theme. A new CI workflow builds with `--strict` on push to `main` and deploys to the `gh-pages` branch. Content adapts existing skill references while keeping them intact as the canonical agent skill.

**Tech Stack:** Python 3, mkdocs-material, peaceiris/actions-gh-pages

## Global Constraints

- CI actions must be SHA-pinned with version comment
- `mkdocs build --strict` — warnings (broken links, missing pages) fail the build
- Existing `skills/fez/` files remain unchanged
- Existing `docs/agents/` files remain unchanged
- `docs/` becomes mkdocs content root; `docs/agents/` is excluded from the site nav
- README.md unchanged

---

### Task 1: Scaffold mkdocs config and dependencies

**Files:**
- Create: `mkdocs.yml`
- Create: `requirements.txt`

**Interfaces:**
- Produces: `mkdocs build --strict` runs successfully (no pages yet — empty nav is fine, or index placeholder)

- [ ] **Step 1: Create requirements.txt**

```text
mkdocs-material==9.6.*
```

Pin to the latest 9.6.x at the time of writing.

- [ ] **Step 2: Create mkdocs.yml**

```yaml
site_name: fez
site_description: Agent-native management CLI for Fedora/RHEL
repo_url: https://github.com/major/fez
theme:
  name: material
  features:
    - navigation.sections
    - search.highlight
    - search.suggest
strict: true
nav:
  - Home: index.md
  - Getting Started: getting-started.md
  - Agent Guide:
    - Overview: agent-guide/index.md
    - JSON Envelope: agent-guide/json-envelope.md
    - Safety & Guardrails: agent-guide/safety.md
    - Remote Hosts: agent-guide/remote-hosts.md
    - Workflows: agent-guide/workflows.md
```

- [ ] **Step 3: Verify mkdocs builds (with placeholder index)**

```bash
python3 -m venv /tmp/fez-docs-venv
source /tmp/fez-docs-venv/bin/activate
pip install -r requirements.txt
echo '# fez' > docs/index.md
mkdocs build --strict
```

Expected: exit 0, `site/` directory created.

- [ ] **Step 4: Commit**

```bash
git add mkdocs.yml requirements.txt docs/index.md
git commit -m "feat: scaffold mkdocs+material config for docs site"
```

---

### Task 2: Add GitHub Actions docs workflow

**Files:**
- Create: `.github/workflows/docs.yml`

**Interfaces:**
- Consumes: `mkdocs.yml`, `requirements.txt` from Task 1
- Produces: on push to `main`, builds and deploys to `gh-pages` branch

- [ ] **Step 1: Create .github/workflows/docs.yml**

```yaml
name: docs

on:
  push:
    branches:
      - main
    paths:
      - 'docs/**'
      - 'skills/fez/**'
      - 'mkdocs.yml'
      - 'requirements.txt'
      - '.github/workflows/docs.yml'

permissions:
  contents: write

jobs:
  deploy:
    name: deploy
    runs-on: ubuntu-latest
    steps:
      - name: Checkout repository
        uses: actions/checkout@9c091bb21b7c1c1d1991bb908d89e4e9dddfe3e0  # v7.0.0
        with:
          persist-credentials: false

      - name: Setup Python
        uses: actions/setup-python@a8071901898cc51043022d384bc9267e9dd029ef  # v5.7.0
        with:
          python-version: '3.x'

      - name: Install dependencies
        run: pip install -r requirements.txt

      - name: Build docs
        run: mkdocs build --strict

      - name: Deploy to GitHub Pages
        uses: peaceiris/actions-gh-pages@981c28237bcdd3bd01a658beb30595a85c6805af  # v4.0.0
        with:
          github_token: ${{ secrets.GITHUB_TOKEN }}
          publish_dir: ./site
          force_orphan: true
```

- [ ] **Step 2: Verify YAML syntax**

```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/docs.yml'))"
```

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/docs.yml
git commit -m "feat: add GitHub Actions workflow for mkdocs deploy"
```

---

### Task 3: Write landing page (index.md)

**Files:**
- Modify: `docs/index.md` (replace placeholder from Task 1)

**Interfaces:**
- Consumes: none
- Produces: readable landing page that explains what fez is and who it's for

- [ ] **Step 1: Write docs/index.md**

```markdown
# fez

`fez` is an **agent-native** management CLI for Fedora and RHEL. It gives both
LLM-driven agents and humans a uniform, structured, discoverable way to operate
a host — and, over SSH, a fleet — by reusing Cockpit's existing system plumbing
instead of scraping dozens of human-oriented CLIs.

## Who is this for?

- **LLM agents** that need machine-readable output, safe mutation guardrails,
  and a consistent discovery model (`fez describe`, `fez capabilities`).
- **Humans** who want fast system insight without remembering systemctl,
  firewall-cmd, dnf, and nmcli flags.

## Quick example

```bash
# Local host overview
fez system show --json

# Check a service on a remote host
fez --host web1 services status nginx.service --json

# Restart safely: inspect, dry-run, execute
fez describe services.restart --json
fez services restart nginx.service --dry-run --json
fez services restart nginx.service --json
```

## Start here

- **New to fez?** Go through [Getting Started](getting-started.md).
- **Building an LLM agent?** Read the [Agent Guide](agent-guide/index.md).
- **Source:** [github.com/major/fez](https://github.com/major/fez)
```

- [ ] **Step 2: Verify mkdocs builds**

```bash
source /tmp/fez-docs-venv/bin/activate
mkdocs build --strict
```

Expected: exit 0.

- [ ] **Step 3: Commit**

```bash
git add docs/index.md
git commit -m "docs: write landing page"
```

---

### Task 4: Write getting-started guide

**Files:**
- Create: `docs/getting-started.md`

**Interfaces:**
- Consumes: none
- Produces: self-contained getting-started walkthrough

- [ ] **Step 1: Write docs/getting-started.md**

```markdown
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
- [JSON Envelope](agent-guide/json-envelope.md) — deep dive on parsing responses
- [Safety & Guardrails](agent-guide/safety.md) — when and how to use `--force`
```

- [ ] **Step 2: Verify mkdocs builds**

```bash
source /tmp/fez-docs-venv/bin/activate
mkdocs build --strict
```

Expected: exit 0.

- [ ] **Step 3: Commit**

```bash
git add docs/getting-started.md
git commit -m "docs: write getting-started guide"
```

---

### Task 5: Write agent guide overview

**Files:**
- Create: `docs/agent-guide/index.md`

**Interfaces:**
- Source: adapts content from `skills/fez/SKILL.md`
- Produces: agent guide landing page with the operating loop and reference map

- [ ] **Step 1: Write docs/agent-guide/index.md**

```markdown
# Agent Guide

`fez` is built for LLM-driven agents. This guide covers the operating loop,
mental model, and reference material you need to use fez reliably.

## The Operating Loop

Every fez session follows this pattern:

1. **Orient** — get the lay of the land.
2. **Discover** — see what fez can do on this host.
3. **Describe** — inspect a command before using it.
4. **Act** — read or mutate, preferring `--json`.

### 1. Orient

```bash
fez guide --json
fez system show --json
```

`fez guide` prints a compact reference card. `fez system show` gives you host
state: OS release, hostname, uptime, kernel, and which subsystems are
reachable.

### 2. Discover

```bash
fez capabilities
```

Lists every capability fez can execute against the current host. Capabilities
vary by host — a target without firewalld won't show firewall commands.

### 3. Describe

```bash
fez describe services.status --json
fez describe packages.install --json
```

Descriptors tell you inputs, flags, output kind, privilege requirements,
examples, and JSON schema. Always describe an unfamiliar command before using
it.

### 4. Act

```bash
# Read-only (no elevation needed)
fez services list --state failed --json
fez services status sshd.service --json

# Mutation (privileged, guarded by safety layer)
fez services restart nginx.service --dry-run --json
fez services restart nginx.service --json
```

Prefer `--json` in every command. Parse the envelope: check `status` first,
then inspect `data` or `error` + `hints`.

## Reference Map

| Topic | Page |
|-------|------|
| Envelope format, exit codes, hints | [JSON Envelope](json-envelope.md) |
| Guardrails, `--dry-run`, `--force` | [Safety & Guardrails](safety.md) |
| SSH transport, `--host`, identities | [Remote Hosts](remote-hosts.md) |
| End-to-end workflow examples | [Workflows](workflows.md) |

## Red Flags

Stop and inspect `fez guide --json` or `fez describe <id> --json` if you are:

- Guessing flags instead of reading the descriptor
- Parsing human-readable output when `--json` is available
- Ignoring the `hints` field on error responses
- Using `--force` without a specific guardrail reason
- Changing a remote host without an explicit `--host` target
```

- [ ] **Step 2: Verify mkdocs builds**

```bash
source /tmp/fez-docs-venv/bin/activate
mkdocs build --strict
```

Expected: exit 0, no broken links.

- [ ] **Step 3: Commit**

```bash
git add docs/agent-guide/index.md
git commit -m "docs: write agent guide overview"
```

---

### Task 6: Write JSON envelope reference

**Files:**
- Create: `docs/agent-guide/json-envelope.md`

**Interfaces:**
- Source: adapts from `skills/fez/references/safety-and-errors.md`
- Produces: envelope anatomy, exit code table, hints usage

- [ ] **Step 1: Write docs/agent-guide/json-envelope.md**

```markdown
# JSON Envelope

Every `--json` response uses the compact `fez/v1` envelope. This page covers
the envelope structure, exit codes, and how to parse responses.

## Envelope Anatomy

Success:

```json
{"apiVersion":"fez/v1","kind":"SystemOverview","host":"localhost","status":"ok","data":{}}
```

Error:

```json
{"apiVersion":"fez/v1","kind":"Error","host":"localhost","status":"error","error":{"code":"dependency-missing","message":"required target dependency is missing"},"hints":{"remediation":"install the missing target package"}}
```

### Fields

| Field | Always present? | Description |
|-------|:---:|---|
| `apiVersion` | Yes | Always `"fez/v1"` |
| `kind` | Yes | The response type: `"ServiceList"`, `"SystemOverview"`, `"Error"`, etc. |
| `host` | Yes | The target host (`"localhost"` or a remote hostname) |
| `status` | Yes | `"ok"` or `"error"`. Check this first. |
| `data` | On success | The payload. Shape depends on `kind`. |
| `error` | On error | Object with `code`, `message`, and optional `detail`. |
| `hints` | On error | Actionable remediation: package names, flag suggestions, next steps. |

## Parsing Flow

1. Parse the line as JSON.
2. Check `status`.
3. If `"ok"`: read `data` according to `kind`.
4. If `"error"`: read `error.code` for programmatic handling, `error.message` for display, and `hints` for remediation.
5. Always surface `hints` to the user — they are target-specific and more reliable than guessing.

## Exit Codes

Fez exits with these codes. The envelope's `error.code` field is the
machine-readable equivalent.

| Exit | Label | Meaning |
| ---: | --- | --- |
| 0 | success | Command completed successfully. |
| 1 | general | Unclassified failure (I/O, decode, aborted operation). |
| 2 | usage | CLI usage error: missing argument, invalid argument, unknown flag. |
| 4 | not-found | Target resource does not exist. |
| 5 | timeout | The bridge did not respond before the deadline. |
| 6 | bridge | Bridge could not be spawned or the connection closed. |
| 7 | dbus | A D-Bus call returned an error. |
| 8 | protected-unit | Protected operation refused without `--force`. |
| 9 | dependency-missing | Required target dependency is absent or not activatable. |
| 10 | dangerous-transaction | Resolved transaction refused by guardrails without `--force`. |
| 11 | access-denied | Privilege escalation failed or was denied. |
| 12 | unsupported-api | Managed subsystem is reachable but lacks a required D-Bus method. |

## Hints

The `hints` field is the first remediation source. Examples:

- `"remediation": "install the dnf5daemon-server package"` — a missing dependency
- `"remediation": "use --force to bypass the guarded operation"` — a safety block
- `"remediation": "cockpit escalation failed; check polkit rules or sudo access"` — privilege denied

Never invent package names or fallback commands when `hints` gives
target-specific remediation.
```

- [ ] **Step 2: Verify mkdocs builds**

```bash
source /tmp/fez-docs-venv/bin/activate
mkdocs build --strict
```

Expected: exit 0.

- [ ] **Step 3: Commit**

```bash
git add docs/agent-guide/json-envelope.md
git commit -m "docs: write JSON envelope reference"
```

---

### Task 7: Write safety & guardrails reference

**Files:**
- Create: `docs/agent-guide/safety.md`

**Interfaces:**
- Source: adapts from `skills/fez/references/safety-and-errors.md`
- Produces: guardrails philosophy, mutation workflow, `--force` rules, concrete examples

- [ ] **Step 1: Write docs/agent-guide/safety.md**

```markdown
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
```

- [ ] **Step 2: Verify mkdocs builds**

```bash
source /tmp/fez-docs-venv/bin/activate
mkdocs build --strict
```

Expected: exit 0.

- [ ] **Step 3: Commit**

```bash
git add docs/agent-guide/safety.md
git commit -m "docs: write safety and guardrails reference"
```

---

### Task 8: Write remote hosts reference

**Files:**
- Create: `docs/agent-guide/remote-hosts.md`

**Interfaces:**
- Source: adapts from `skills/fez/references/remote-hosts.md`
- Produces: SSH transport, `--host`, identities, capability-to-dependency mapping

- [ ] **Step 1: Write docs/agent-guide/remote-hosts.md**

```markdown
# Remote Hosts

Fez reaches remote hosts by shelling out to the system OpenSSH client. This
page covers targeting, SSH configuration, and target requirements.

## Targeting a Remote Host

Use `--host` with a hostname or SSH config alias:

```bash
fez --host web1 system show --json
fez --host web1 services status nginx.service --json
fez --host web1 services restart nginx.service --dry-run --json
```

If the user names a remote host, include `--host`. Do not silently run the
command against localhost. Without `--host`, fez operates on the local machine.

## SSH Configuration

Fez uses the system OpenSSH client (`ssh`). SSH config aliases defined in
`~/.ssh/config` work automatically. Use `FEZ_SSH_CONFIG` to point fez at a
specific config file:

```bash
FEZ_SSH_CONFIG=/path/to/ssh_config fez --host web1 system show --json
```

## Explicit Identities

By default, fez lets OpenSSH use its normal identity and agent behavior
(`IdentitiesOnly=no`). Add `--ssh-identities-only` only when the user or
environment requires `IdentitiesOnly=yes`:

```bash
fez --host web1 --ssh-identities-only system show --json
```

The equivalent environment variable:

```bash
FEZ_SSH_IDENTITIES_ONLY=1 fez --host web1 system show --json
```

## Target Requirements

The remote host needs `cockpit-bridge` installed. Some capabilities need
additional target packages:

| Capability | Target dependency |
| --- | --- |
| `fez system metrics` | `pcp` (Performance Co-Pilot) |
| `fez system firmware` | `fwupd` |
| `fez system subscription` | `subscription-manager` (RHEL) |
| `fez firewall` | `firewalld` |
| `fez storage` | `udisks2` |
| `fez dns status` | `systemd-resolved` (preferred) or NetworkManager |
| `fez packages` | `dnf5daemon` (preferred) or PackageKit |

When a dependency is missing, the JSON `hints` field gives the specific
package to install. Prefer hints over guessing.
```

- [ ] **Step 2: Verify mkdocs builds**

```bash
source /tmp/fez-docs-venv/bin/activate
mkdocs build --strict
```

Expected: exit 0.

- [ ] **Step 3: Commit**

```bash
git add docs/agent-guide/remote-hosts.md
git commit -m "docs: write remote hosts reference"
```

---

### Task 9: Write workflows reference

**Files:**
- Create: `docs/agent-guide/workflows.md`

**Interfaces:**
- Source: adapts from `skills/fez/references/workflow.md`
- Produces: end-to-end workflow examples for common LLM tasks

- [ ] **Step 1: Write docs/agent-guide/workflows.md**

```markdown
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
```

- [ ] **Step 2: Verify mkdocs builds**

```bash
source /tmp/fez-docs-venv/bin/activate
mkdocs build --strict
```

Expected: exit 0.

- [ ] **Step 3: Commit**

```bash
git add docs/agent-guide/workflows.md
git commit -m "docs: write agent workflows reference"
```

---

### Task 10: Update AGENTS.md with docs maintenance reminder

**Files:**
- Modify: `AGENTS.md`

**Interfaces:**
- Produces: reminder to update docs and skill references when project changes

- [ ] **Step 1: Add reminder to AGENTS.md conventions section**

Append to the Conventions section (after the last bullet, before the next heading):

```markdown
- When a feature changes the CLI surface, output format, safety behavior, or
  agent workflow, update the corresponding page in `docs/` and the skill
  references in `skills/fez/references/` in the same commit.
```

The edit target is this region of AGENTS.md:

```markdown
- CLI serialization and bridge request bodies have different `None`/default semantics; test them separately.
- README links a missing design spec at `docs/superpowers/specs/2026-06-09-agentic-os-design.md`; do not trust that link until it exists.
```

The new bullet goes between the existing last convention bullet and `## Env Vars`.

- [ ] **Step 2: Verify no diff noise**

```bash
git diff AGENTS.md
```

Expected: one new bullet added.

- [ ] **Step 3: Commit**

```bash
git add AGENTS.md
git commit -m "docs: add reminder to keep docs and skill refs current"
```

---

### Post-Implementation: Enable GitHub Pages

After all commits are merged to `main` and the `docs.yml` workflow deploys the `gh-pages` branch:

1. Go to repo Settings → Pages.
2. Set source to "Deploy from a branch".
3. Select `gh-pages` branch, root directory (`/`).
4. Save. The site will be live at `https://major.github.io/fez`.
