# CLI Contract Notes For Agents

Read this before changing `src/cli.rs`, `src/schema/`, `src/error.rs`, help text, examples, man output, or CLI tests.

## Clap Entry Points

`src/cli.rs` owns the command line surface:

- `command()` builds the registry-enriched clap tree.
- `raw_command()` builds the bare derived tree.
- `parse_or_render()` is the binary entry point. It calls `try_get_matches`, renders `--help` and `--version` itself, and turns clap usage errors into envelopes when `--json` is present.

Top-level verbs beyond capability commands:

- `fez guide` renders the agent contract; `--json` emits an `AgentGuide` envelope.
- Hidden `fez man` emits the `fez.1` roff page for packaging.

## Envelope And Exit Codes

`src/envelope.rs` owns the compact `fez/v1` JSON response envelope. `Envelope::to_json_string` uses `serde_json::to_string`, not pretty printing.

`src/error.rs` owns `FezError`, stable string `code()` values, `exit_code()` mappings, structured `detail()` payloads, and the `EXIT_CODES` table. Exit codes are part of the contract. `detail()` is the structured `fez/v1` error-envelope payload and is currently present for `DependencyMissing`, `DangerousTransaction`, and `UnsupportedApi`; keep it co-located with `code()` and `exit_code()` so they cannot drift. Common mappings include:

| Code | Exit |
| --- | --- |
| `usage` | 2 |
| `not-found` | 4 |
| `timeout` | 5 |
| `bridge-spawn` / `bridge-closed` | 6 |
| `dbus` | 7 |
| `protected-unit` | 8 |
| `dependency-missing` | 9 |
| `dangerous-transaction` | 10 |
| `access-denied` | 11 |
| `unsupported-api` | 12 |

The `usage` code covers CLI parse failures rendered as JSON. Clap surfaces these before a `FezError` exists, so `parse_or_render()` builds the envelope directly while `FezError::Usage` carries the same code and exit for tables and tests.

E2E and integration tests assert on exit codes. Update tests when touching mappings.

## Capability Registry

The capability registry in `src/schema/mod.rs` is canonical. `src/schema/help.rs::inject()` walks the derived clap tree and attaches each descriptor's `long` and `examples` as `long_about` and `after_help`.

When adding or changing a command, flag, or argument, update both:

- The clap `about` and argument definitions.
- The matching descriptor summary, long text, inputs, flags, examples, and schema.

`fez describe <id>` renders from the descriptor in both formats:

- `--json` serializes the descriptor.
- Plain text uses `Descriptor::render_text`, including summary, long text, privilege status, output, inputs, flags, and examples.

`fez describe <id> --json` emits the compatibility `flags: ["--flag"]` list and computed `flag_schema` entries with `type`, `description`, `repeatable`, `default`, and `conflicts_with` metadata. Constrained positional inputs use `choices`.

## Help, Describe, Guide, And Man

Help, `fez describe`, and `fez man` derive from descriptors. Drift guards in `tests/cli.rs` lock down descriptor path/id mapping, examples, safety-global visibility, and exit-code tables.

The `services <action>` to `services.<action>` path/id mapping is covered by drift tests. Keep new descriptors consistent with that convention.

## Global Flags

`--dry-run`, `--force`, and `--ssh-identities-only` are clap globals on the root `Cli`, so clap accepts them on every subcommand. `--ssh-identities-only` is a transport knob: it opts SSH connections into OpenSSH `IdentitiesOnly=yes`; by default fez omits that option so agent-backed keys (for example 1Password SSH agent keys) work like normal `ssh`.

`inject()` hides a safety global from a leaf's help when that leaf's descriptor does not advertise it. The hide is help-only: a hidden global still parses as a no-op for read-only commands. This preserves the global CLI contract while keeping leaf help aligned with descriptors.

The implementation uses local hidden non-global shadow args with the same id because clap propagates globals lazily and child commands cannot reliably reach them with `mut_arg`.

The root `--force` help is intentionally generic: "Override command-specific safety guardrails. See command help for exact risks." Exact risk wording belongs in each descriptor's long text.
