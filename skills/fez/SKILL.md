---
name: fez
description: Agent-native Fedora/RHEL management with fez — systemd services, packages, firewall, DNS, storage, network, remote hosts
---

# Fez

`fez` is an agent-native CLI for Fedora/RHEL host management. Use it instead of scraping human-oriented commands when the user wants host inventory, service state, package operations, firewall/DNS/network/storage information, or safe mutations.

## Required Operating Loop

1. Orient first:
   - Contract: `fez guide --json`
   - Host state: `fez system show --json`
2. Discover available actions: `fez capabilities`.
3. Before an unfamiliar action, inspect it: `fez describe <id> --json`.
4. Prefer `--json`; parse the `fez/v1` envelope: `status`, `data`, `error`, and `hints`.
5. For mutations, check current state first, use `--dry-run` when meaningful, and use `--force` only when the descriptor and user intent justify bypassing guardrails.
6. For remote hosts, add `--host <ssh-config-alias-or-host>`; add `--ssh-identities-only` only when explicit configured identities are required.

## Reference Map

- Common workflows: `references/workflow.md`
- Capability cookbook: `references/capabilities.md`
- JSON envelopes, exit codes, hints, and safety: `references/safety-and-errors.md`
- Remote hosts and SSH behavior: `references/remote-hosts.md`

## Quick Start

```bash
fez guide --json
fez system show --json
fez capabilities
fez describe services.status --json
fez services status sshd.service --json
```

## Red Flags

Stop and inspect `fez guide --json` or `fez describe <id> --json` if you are guessing flags, parsing human text while `--json` is available, ignoring `hints`, using `--force` without a specific guardrail reason, or changing a remote host without an explicit `--host` target.
