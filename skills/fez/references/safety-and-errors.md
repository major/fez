# Fez Safety and Error Reference

## JSON Envelope

Every `--json` response uses the compact `fez/v1` envelope:

```json
{"apiVersion":"fez/v1","kind":"SystemOverview","host":"localhost","status":"ok","data":{}}
```

Error responses use `status:"error"`:

```json
{"apiVersion":"fez/v1","kind":"Error","host":"localhost","status":"error","error":{"code":"dependency-missing","message":"required target dependency is missing"},"hints":{"remediation":"install the missing target package"}}
```

Read `status` first. On success, inspect `data`. On error, inspect `error.code`, `error.message`, optional `error.detail`, and `hints`.

## Exit Codes

| Exit | Label | Meaning |
| ---: | --- | --- |
| 1 | general | Unclassified failure such as I/O, decode, or aborted operation. |
| 2 | usage | CLI usage error, missing argument, invalid argument, or unknown flag. |
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

Use `hints` as the first remediation source. Do not invent package names or fallback commands when `hints` gives target-specific remediation.

## Mutations

Mutation workflow:

1. Read current state.
2. Inspect descriptor: `fez describe <mutation-id> --json`.
3. Dry-run when meaningful: `--dry-run --json`.
4. Execute only when the requested change and target are clear.
5. Verify with a read-only command.

## Force

`--force` bypasses command-specific guardrails. Use it only when all are true:

- the user explicitly asked for the risky operation or confirmed it after seeing the risk;
- `fez describe <id> --json` shows the command and flags involved;
- a dry-run or read-only check confirms the target resource;
- you can explain the guardrail being bypassed.

Concrete guardrail examples (patterns, not exhaustive):

- **Protected services**: operations on sshd.service, sshd.socket, ssh.service, ssh.socket, cockpit*, and fez* require `--force`.
- **Dangerous packages**: removing kernel*, systemd*, glibc, dnf*, rpm*, sudo, openssh-server, cockpit*, dbus*, or transactions with large cascading removals trigger guardrails.
- **Firewall lockout risks**: removing the current SSH service/port, changing the default zone, enabling panic mode, reloading with drift, or disabling masquerade require `--force`.
- **System power actions**: reboot, poweroff, and halt require `--force`.

## Privilege Escalation

Privileged operations escalate through cockpit. `fez` does not handle sudo passwords. `access-denied` often means the target lacks a usable cockpit escalation mechanism or policy allows discovery but denies mutation.
