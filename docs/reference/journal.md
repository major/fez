# Journal

Query the systemd journal: search entries, discover boot IDs, and list available
fields.

**Backend:** `journalctl` spawned on the target host via cockpit-bridge's stream
channel. No escalation needed (reads journal as the connected user). No extra
packages beyond systemd itself.

## Reading Entries

```
fez journal --json                         # last 25 entries
fez journal --unit sshd.service --lines 50
fez journal --priority err --since '1 hour ago'
fez journal --boot --grep 'Failed password'
fez journal --unit sshd --output-fields _COMM,_EXE --json
```

| Flag | Purpose |
|------|---------|
| `--unit <u>` | Filter by systemd unit (repeatable for multiple units) |
| `--since <expr>` | Entries since this journalctl time expression |
| `--until <expr>` | Entries until this journalctl time expression |
| `--priority <p>` | Minimum priority: `emerg`, `alert`, `crit`, `err`, `warning`, `notice`, `info`, `debug`, or `0`-`7` |
| `--boot` | Restrict to a specific boot (omit value for current boot, or pass a boot ID/offset) |
| `--grep <pattern>` | Filter messages by PCRE regex pattern (server-side) |
| `--lines <n>` | Maximum entries to return (default 25) |
| `--output-fields <fields>` | Additional journal fields to include (comma-separated) |

### Output

Output kind: `JournalEntries`. Default fields per entry:

```
timestamp, hostname, identifier, pid, priority, message
```

`--output-fields` adds to this set (never replaces). Extra fields appear as
named values alongside the standard fields in JSON output, and in brackets
after the message in plain-text output.

### Pagination and Truncation

The default limit is 25 entries. When more entries exist than `--lines`, the
envelope includes `"truncated": true` and a hint suggesting narrower filters:

- Use `--since` / `--until` to narrow the time window
- Use `--grep` to match specific messages
- Use `--priority` to raise the severity threshold
- Increase `--lines` for larger windows

## Discovery

### `fez journal --list-boots`

List available boot IDs for use with `--boot`.

```
fez journal --list-boots
```

Output kind: `JournalBoots`. Each entry has a boot ID string and timestamps
for the first and last entry in that boot.

### `fez journal --list-fields`

List all journal field names available for use with `--output-fields`.

```
fez journal --list-fields
```

Output kind: `JournalFields`. Each entry is a field name (e.g. `_COMM`, `_EXE`,
`_UID`, `_GID`, `SYSLOG_IDENTIFIER`, etc.).

`--list-boots` and `--list-fields` conflict with all filtering and formatting
flags — they are standalone discovery commands.

## Time Expressions

The `--since` and `--until` flags accept standard journalctl time syntax:

```
--since '1 hour ago'
--since '2026-07-01'
--since 'yesterday'
--since '2026-07-01 14:30:00'
```

## Priority Levels

Priorities from most severe to least:

| Priority | Numeric |
|----------|:-------:|
| `emerg` | 0 |
| `alert` | 1 |
| `crit` | 2 |
| `err` | 3 |
| `warning` | 4 |
| `notice` | 5 |
| `info` | 6 |
| `debug` | 7 |

Use the name or number interchangeably. `--priority err` matches entries at
priority 3 and above (more severe).
