# Capability Notes For Agents

Read this before changing `src/capabilities/`, `src/protocol/`, `src/transport/`, `src/bin/fake_bridge.rs`, privilege escalation, audit behavior, or capability integration tests.

## Module Map

- `protocol/` - bridge wire protocol: framed JSON, message types including `DbusSignal`, `dbus_call` request/reply flow, and `dbus_call_collect` for signal-driven PackageKit calls.
- `transport/` - `local` spawns the bridge; `ssh` shells out to the system OpenSSH client.
- `capabilities/` - command implementations for services, packages, network, and firewall.
- `capabilities/mod.rs` - shared `View` result and `render`/`render_with_hints` envelope shaping.
- `dispatch.rs` - routes parsed CLI commands to capabilities.
- `safety.rs` - guardrails for protected units, dangerous package removals, and firewall lockout-prone actions.
- `audit.rs` - append-only JSON-lines audit log. Each executed mutation writes attempt and result records.

## Shared Rendering

Every capability returns `Result<View>` and routes dispatch through the shared renderer. `View` is the render-ready result: `kind`, `host`, `data`, `human`, optional `hints`, and a `pre_rendered` flag. Build it with `View::new(...)`, then `.with_hints`, `.with_hints_opt`, or `.pre_rendered()` as needed.

`render(cli, Result<View>)` is the single stdout/envelope path. The error arm pulls structured detail from `FezError::detail()`, keeping envelope shaping in one place instead of one copy per capability. Capabilities that need safe read-only follow-up hints on error envelopes, currently firewall, route through `render_with_hints(cli, result, hint_fn)` so domain-specific hint wording stays in the capability rather than the shared error type.

## Audit

Mutating capabilities wrap privileged actions in `audit::run_audited(host, operation, unit, || ...)`. It writes the `attempt` record, runs the closure, then writes the `ok` or `error` result record, so the two-record pattern lives in one place.

`run_audited_with(&dyn AuditSink, ...)` is the sink-injected core. `run_audited` is the `sink_from_env` wrapper; unit tests drive the injected form.

## MCP

`fez mcp` runs a JSON-RPC 2.0 MCP server over stdio. Default mode stays frugal and advertises only `list_capabilities`, `describe_capability`, and `invoke`.

`fez mcp --expanded-tools` additionally advertises one strict JSON-schema tool per capability while keeping the three meta-tools available. Expanded tool names map capability IDs by replacing dots with underscores, such as `services.status` to `services_status`. Expanded tools use descriptor inputs and flag metadata for required fields, enum values, and dry-run/force descriptions, then dispatch through the same invoke/re-exec path.

A server launched with global `--host <host>` uses that resolved host as the default target for `invoke` calls that omit `arguments.host`; a per-call `arguments.host` still overrides it. The default target is exposed in `initialize.serverInfo.defaultTargetHost` and the `invoke` tool description from `tools/list`.

## Shared Capability Rules

- `fez` owns no persistent state. Recompute live state or delegate persistence to the managed subsystem.
- Read-only operations should stay unprivileged unless the underlying subsystem gates a required read.
- Mutations that need privileges open privileged channels through cockpit escalation.
- Errors must map to stable `FezError` codes and exits documented in `docs/agents/cli-contract.md`.
- `--json` payloads use compact `fez/v1` envelopes.

## Privilege Escalation

The fake bridge models transparent escalation over `cockpit.Superuser`. `fez` sends bridge `init` with `superuser: "none"`, deferring escalation. Real cockpit does not emit `superuser-init-done` for that bare init, so the client treats the bridge `init` reply as handshake-complete.

Later, the client reads the internal `cockpit.Superuser` `Bridges` property and calls `Start(name)` until a mechanism succeeds. The `Bridges` property arrives as a D-Bus variant-wrapped array (`{"t":"as","v":[...]}`); clients must unwrap it through `variant_value`.

`FEZ_FAKE_BRIDGES` configures fake mechanisms as an ordered list like `sudo:ok` or `sudo:err,polkit:ok`. Unset defaults to `sudo:ok`; an explicitly empty value advertises no mechanism and privileged operations fail with exit 11. `FEZ_FAKE_DENY_PRIVILEGED` models a host where escalation succeeds but the privileged channel is later rejected.

Real standalone `cockpit-bridge` without `cockpit-system` advertises zero superuser bridges. Escalation requires `cockpit-system` plus passwordless sudo or a suitable polkit rule. `fez` does not supply sudo passwords.

`FEZ_ESCALATION=off` disables escalation. Any other value forces that single mechanism with no fall-through.

## Services

The services capability talks to systemd over the bridge. Guardrails in `safety.rs` protect critical units from mutation without `--force`; protected operations return exit 8 (`protected-unit`).

The fake reports `chronyd` inactive and `sshd` active. Tests depend on that canned state.

## Packages

The primary package backend is dnf5daemon (`org.rpm.dnf.v0`). The target must have `dnf5daemon-server` installed and activatable; the package name is not `dnf5daemon`.

Fedora 41+ ships `dnf5daemon-server`. RHEL 10 does not; it keeps dnf4 as the system manager and the dnf5/dnf5daemon stack targets RHEL 11. When dnf5daemon is absent, packages automatically fall back to PackageKit. Only when both dnf5daemon and PackageKit are absent does `fez` return exit 9 with remediation naming both daemons.

dnf5daemon details:

- `packages list --repo` filters client-side on exact `repo_id`; `Rpm.list` has no server-side repo filter.
- Multiple `--repo` flags union.
- The applied repo filter is echoed in the envelope's `repos` field.
- `packages list --name` filters client-side on package-name substring before pagination.
- JSON output includes pagination metadata: `total`, `returned`, `limit`, `offset`, and `next_offset`.
- Unpaginated responses over 1000 rows include a hint recommending `packages search`, `--name`, or `--limit`.
- dnf option dictionaries must use variant-wrapped `a{sv}` values through the `options()` and `variant()` helpers.
- dnf5daemon payloads carry `"backend":"dnf5daemon"`.

PackageKit fallback details:

- Implemented in `src/capabilities/packages_pk.rs` over `org.freedesktop.PackageKit`.
- Automatic and self-configuring; there is no `--backend` flag or env knob.
- Signal-driven flow: `CreateTransaction` returns a transaction object path, then transaction methods emit `Package`, `RepoDetail`, `ErrorCode`, and terminating `Finished` signals collected by `BridgeClient::dbus_call_collect`.
- Reads (`list`, `info`, `search`, `check-update`, `repolist`) run unprivileged.
- Mutations (`install`, `remove`, `upgrade`) open a privileged channel because PackageKit mutation polkit actions are `auth_admin`; root-via-cockpit bypasses that check.
- PackageKit list output applies the same `--repo`, `--name`, `--limit`, and `--offset` client-side semantics and pagination metadata as dnf5daemon.
- PackageKit plans carry no install/download sizes, so size fields are `null`; payloads carry `"backend":"packagekit"` plus a degraded-schema hint.
- The same removal guardrail and audit flow are reused. Protected removals are exit 10 (`dangerous-transaction`).
- PackageKit `NOT_AUTHORIZED` maps to exit 11 (`access-denied`).

## Network

The network capability drives NetworkManager over `org.freedesktop.NetworkManager`. It is read-only and opens unprivileged channels only.

NetworkManager reuses generic `Get` and `GetAll` property methods across object types. The fake dispatches by object path, not just method name, for manager, device, IP config, active connection, and DHCP config objects.

`src/capabilities/network.rs` keeps D-Bus transport values raw only at the call boundary. Device, IP config, and active connection properties are converted into private typed structs before filtering or rendering. DHCP option properties are flattened at the boundary and remain JSON because the option map is arbitrary. Keep new NetworkManager reads inside that boundary instead of spreading `serde_json::Value` indexing through command logic.

Canned topology from `GetDevices`:

- `enp1s0` - ethernet, activated, full IPv4/IPv6/active-connection/DHCP data.
- `enp2s0` - ethernet, unavailable, null `/` IP configs.
- `lo` - loopback, unmanaged but kept by the default type filter.
- `veth0` - veth, unmanaged, hidden by default and shown only with `--all`.

`network list` hides unmanaged virtual interfaces unless `--all`. `network show <device>` looks up by `Interface` name and returns exit 4 (`not-found`) for unknown devices.

## Firewall

The firewall capability drives firewalld over `org.fedoraproject.FirewallD1`. Interface discipline matters:

- Runtime zone reads (`getZones`, `getServices`, `getPorts`, `getInterfaces`, `getSources`) go on `org.fedoraproject.FirewallD1.zone`.
- `getDefaultZone`, `listServices`, `queryPanicMode`, and mutations go on root `org.fedoraproject.FirewallD1`.
- Permanent config reads use `config` / `config.zone` and are polkit-gated.

`list`, `show`, and `services` are fully unprivileged. `status` is mostly unprivileged but escalates for the permanent config read needed to compute runtime-vs-permanent drift. A host with no escalation mechanism fails `status` with exit 11 rather than silently reporting empty drift.

Mutations apply to runtime only. Persistence happens through `fez firewall confirm`, which calls firewalld `runtimeToPermanent`. `status` recomputes drift live each call.

The protected-op guard refuses lockout-prone operations without `--force`:

- Removing the session SSH service or port.
- Any default-zone change.
- `panic on`.
- Drift-discarding `reload`.
- Disabling masquerade. Enabling masquerade is unguarded.

When firewalld is absent or unreachable, `fez` returns exit 9 (`dependency-missing`) with remediation covering both install and enable/start. Firewalld is D-Bus-activated, so absent service and stopped-but-installed are not reliably distinct over the bridge.

Older firewalld APIs that return `UnknownMethod` for a feature, such as `getMasquerade`, map to `FezError::UnsupportedApi` (`unsupported-api`, exit 12) rather than dependency-missing.
