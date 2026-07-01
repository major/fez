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
