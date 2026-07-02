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
| Discovery loop: `fez capabilities`, `fez describe`, `fez guide` | [Discovery & Describe](discovery.md) |
| Envelope format, exit codes, hints | [JSON Envelope](json-envelope.md) |
| Guardrails, `--dry-run`, `--force` | [Safety & Guardrails](safety.md) |
| SSH transport, `--host`, identities | [Remote Hosts](remote-hosts.md) |
| End-to-end workflow examples | [Workflows](workflows.md) |
| Every command fez can run | [Capability Reference](../reference/index.md) |

## Red Flags

Stop and inspect `fez guide --json` or `fez describe <id> --json` if you are:

- Guessing flags instead of reading the descriptor
- Parsing human-readable output when `--json` is available
- Ignoring the `hints` field on error responses
- Using `--force` without a specific guardrail reason
- Changing a remote host without an explicit `--host` target
