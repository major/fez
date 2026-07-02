# Discovery & Describe

Fez is designed to be explored without reading docs. Three commands form
the discovery loop — every agent and human session should start here.

## The Loop

```text
fez capabilities            list every capability id
fez describe <id> --json    inspect a command before using it
fez <command> ... --json    invoke it, parse the fez/v1 envelope
```

No guessing flags. No guessing output shapes. No guessing privilege needs.

## `fez guide`

`fez guide` prints a compact reference card covering the entire operating
contract in one call:

```bash
fez guide --json
```

The output includes:

- Orientation command (`fez system show --json`)
- Discovery loop steps
- Envelope anatomy (`apiVersion`, `kind`, `host`, `status`, `data`, `error`, `hints`)
- Global flags (`--host`, `--json`, `--dry-run`, `--force`, `--ssh-identities-only`)
- All exit codes with labels and meanings
- Relevant environment variables

Run `fez guide --json` at the start of any session to refresh the contract.

## `fez capabilities`

Lists every capability id the current fez binary supports:

```bash
fez capabilities
```

Output is one id per line, or a JSON envelope under `--json`:

```json
{"apiVersion":"fez/v1","kind":"CapabilityList","host":"localhost","status":"ok","data":{"capabilities":["services.list","services.status",...]}}
```

Capability ids are stable dotted paths (e.g. `services.start`, `packages.info`,
`firewall.add-service`). Use them verbatim with `fez describe`.

## `fez describe`

Inspects a single capability id before invocation:

```bash
fez describe services.restart --json
fez describe packages.install --json
```

The descriptor tells you everything needed to use the command safely:

| Field | Content |
|-------|---------|
| `id` | The dotted capability id |
| `summary` | One-line purpose |
| `long` | Full description with behavior notes |
| `privileged` | Whether escalation is needed |
| `output_kind` | The envelope `kind` on success |
| `output.schema` | JSON Schema for `data` |
| `output.error` | JSON Schema for `error` |
| `output.error_envelope` | JSON Schema for the full error envelope |
| `inputs` | Named arguments (required/optional, type, default, choices) |
| `flags` | Accepted flags |
| `flag_schema` | Flag types, defaults, and descriptions |
| `examples` | Ready-to-run example invocations |

Without `--json`, `fez describe` prints a compact text form with the same
metadata — inputs, output kind, flags, privileged status, and examples.

### Descriptor Contract

- Descriptors are **read-only and pure**: they never open a transport or touch
  the target host. `fez describe` is safe to run against an unreachable host to
  learn what a command would do.
- Descriptors include `--host` and `--json` in `flags` even though those are
  global; they omit `--dry-run` and `--force` when the command is read-only.
- The `output_kind` field is the same string that appears in the `kind` field
  of a success envelope. Match on it to distinguish response shapes.

## Discovery in Practice

Start every session:

```bash
fez guide --json           # refresh the contract
fez system show --json     # orient to the host
```

Before any unfamiliar command:

```bash
fez describe <id> --json   # read inputs, flags, and output kind
```

Only then invoke the command. Never guess flags or output shapes when the
descriptor is one command away.
