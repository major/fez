# Capability Notes For Agents

Read this before changing `src/capabilities/`, `src/protocol/`, `src/transport/`, `src/bin/fake_bridge.rs`, privilege escalation, audit behavior, or capability integration tests.

## Module Map

- `protocol/` - bridge wire protocol: framed JSON, message types including `DbusSignal`, `dbus_call` request/reply flow, and `dbus_call_collect` for signal-driven PackageKit calls.
- `transport/` - `local` spawns the bridge; `ssh` shells out to the system OpenSSH client.
- `capabilities/` - command implementations for services, packages, network, firewall, and system.
- `capabilities/mod.rs` - shared `View` result and `render` envelope shaping.
- `dispatch.rs` - routes parsed CLI commands to capabilities.
- `safety.rs` - guardrails for protected units, dangerous package removals, and firewall lockout-prone actions.
- `audit.rs` - append-only JSON-lines audit log. Each executed mutation writes attempt and result records.

## Shared Rendering

Every capability returns `Result<View>` and routes dispatch through the shared renderer. `View` is the render-ready result: `kind`, `host`, `data`, `human`, optional `hints`, and a `pre_rendered` flag. Build it with `View::new(...)`, then `.with_hints`, `.with_hints_opt`, or `.pre_rendered()` as needed.

`render(cli, Result<View>)` is the single stdout/envelope path. The error arm pulls structured detail from `FezError::detail()` and actionable follow-up hints from `FezError::hints()`, keeping envelope shaping in one place instead of one copy per capability. `FezError::hints()` is the canonical hint contract: `DependencyMissing` errors expose their `remediation` field; `UnsupportedApi` errors note the missing method. All capabilities benefit from this uniformly — no per-capability hint hook is needed.

## Audit

Mutating capabilities wrap privileged actions in `audit::run_audited(host, operation, unit, || ...)`. It writes the `attempt` record, runs the closure, then writes the `ok` or `error` result record, so the two-record pattern lives in one place.

`run_audited_with(&dyn AuditSink, ...)` is the sink-injected core. `run_audited` is the `sink_from_env` wrapper; unit tests drive the injected form.

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

`FEZ_ESCALATION=off` disables escalation. Any other non-empty value forces that single mechanism with no fall-through only when it is a safe known mechanism advertised by the bridge.

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

## System

The system capability gathers a host overview from two universally available systemd D-Bus services: `org.freedesktop.hostname1` and `org.freedesktop.timedate1`. Both are part of systemd itself, so they require zero extra packages on Fedora or RHEL.

`system show` calls `hostname1.Describe()`, which returns a JSON string containing hostname, OS, kernel, hardware, and firmware fields, then `timedate1.GetAll` for timezone, NTP, and clock data. Both are read-only and unprivileged.

`hostname1.Describe()` returns a single `s` D-Bus out-arg containing a JSON object as a string (not a variant-wrapped `a{sv}` dict). The capability parses the inner JSON string to extract typed fields. The `OperatingSystemReleaseData` array of `KEY=VALUE` strings is parsed into a flat `os_release` object with lowercase keys so agents can read `id`, `version_id`, `variant_id` etc. directly.

Canned fake bridge data: hostname `testbox.example.com`, Fedora 44 Server, QEMU VM, America/Chicago timezone, NTP synchronized. Tests depend on that canned state.

### System Metrics (PCP)

`system metrics` gathers a one-shot performance snapshot from PCP through cockpit-bridge's `metrics1` channel. It opens a `direct` source (local PCP context, no pmcd daemon required), collects 2 samples at 1-second intervals (rate metrics need a delta), and returns CPU, memory, load, disk I/O, and per-interface network throughput.

Requires `pcp` and `python3-pcp` on the target. When PCP is absent, the bridge closes the channel with `not-supported` and fez returns exit 9 with remediation naming both packages.

The real cockpit-bridge ignores `limit` for direct sources and streams indefinitely. The client closes the channel itself once enough samples are collected.

Canned fake bridge data: 3-interface topology (`lo`, `enp1s0`, `enp2s0`), ~3.2% CPU, ~26.8% memory usage, 42.5 disk IOPS. Tests depend on that canned state.

### Sessions and Users (logind)

`system sessions` lists active login sessions from systemd-logind (`org.freedesktop.login1`). Uses `ListSessions` to enumerate sessions, then `GetAll` per session for full detail (type, remote host, state, service, class). Session counts are always small (<10), so the per-session calls are negligible.

`system users` lists logged-in users via `ListUsers`. `system inhibitors` lists shutdown/sleep inhibitors via `ListInhibitors`. `system boot-entries` reads the `BootLoaderEntries` property. All four are unprivileged reads.

Canned fake bridge data: 2 sessions (SSH + local TTY), 2 users (major + root), 1 inhibitor (NetworkManager sleep delay), 2 boot entries. Tests depend on that canned state.

### Power Actions (logind)

`system reboot`, `system poweroff`, and `system suspend` call `login1.Reboot(true)`, `PowerOff(true)`, and `Suspend(true)` respectively. All are protected operations requiring `--force` (exit 8 without it). `CanReboot`/`CanPowerOff`/`CanSuspend` is checked first; `"na"` → exit 9 with remediation. Privileged: requires cockpit escalation. Audited as `system-reboot`, `system-poweroff`, `system-suspend`.

Canned fake: `CanReboot` = `"yes"`, `CanPowerOff` = `"yes"`, `CanSuspend` = `"na"`. Reboot/PowerOff succeed (no-op). Tests depend on that canned state.

## Subscription (RHSM)

The subscription capability reads RHEL subscription status from `com.redhat.RHSM1`. RHEL only: absent on Fedora (exit 9).

`system subscription` calls four RHSM interfaces:

- `Consumer.GetUuid("")` for consumer UUID
- `Entitlement.GetStatus("", "")` for entitlement status
- `Products.ListInstalledProducts("", {}, "")` for installed products
- `Syspurpose.GetSyspurpose("")` for system purpose

All methods take a locale string as the last argument (pass `""`). RHSM methods return JSON-encoded strings as D-Bus `s` values; the inner JSON is parsed.

Canned fake bridge data: UUID `12345678-abcd-...`, status "Current", 1 product (RHEL 10 x86_64), syspurpose role "Red Hat Enterprise Linux Server". `FEZ_FAKE_NO_RHSM=1` → ServiceUnknown. Tests depend on that canned state.

## Firmware (fwupd)

The firmware capability reads device and security data from `org.freedesktop.fwupd`. Read-only: no install/update mutations.

Three actions: `system firmware list`, `system firmware security`, `system firmware upgrades`.

`GetDevices` returns `aa{sv}`. The `Flags` field is a bitmask; bit 2 (`0x4`) indicates the device is updatable. `GetHostSecurityAttrs` returns the HSI security attributes. `GetUpgrades(device_id)` returns available upgrades per device; called only for updatable devices.

The `HostSecurityId` property (e.g. `"HSI:1 (v2.1.3)"`) is read via `Properties.Get`.

fwupd is polkit-gated. `fez` tries unprivileged first; absent service → exit 9 with remediation.

Canned fake bridge data: 2 devices (UEFI updatable, System Firmware not), 3 security attrs (TPM, SecureBoot, IOMMU), 1 upgrade for the UEFI device. `FEZ_FAKE_NO_FWUPD=1` → ServiceUnknown. Tests depend on that canned state.

## DNS

The DNS capability has two backends, selected automatically:

**Primary: systemd-resolved** (`org.freedesktop.resolve1`) — full resolver config, cache stats, DNSSEC/DoT status, per-link detail, cache flush, and hostname resolution. Present on Fedora 41+.

**Fallback: NetworkManager DnsManager** (`org.freedesktop.NetworkManager.DnsManager`) — basic DNS server list, mode, and per-interface config. Used automatically when systemd-resolved is absent (e.g. RHEL 10). Flush and query are unavailable on the fallback and return exit 9 with remediation.

Three actions: `dns status`, `dns flush`, and `dns query`.

`dns status` on resolve1 shows global config (DNS servers, DNSSEC, DNS-over-TLS, LLMNR, multicast DNS, resolv.conf mode, cache statistics) plus per-link DNS detail. Default output hides links without DNS servers; `--all` shows every link. On the NM fallback, shows mode, resolv.conf manager, and per-interface DNS servers. The envelope carries `"backend":"networkmanager"` and a hint noting the reduced feature set.

Link objects on resolve1 live at `/org/freedesktop/resolve1/link/<encoded-ifindex>`. Node names use D-Bus label encoding: the leading digit is underscore + two hex chars (ASCII value), remaining chars literal. Link 2 → `_32`, link 14 → `_314`, link 130 → `_3130`. Enumerated via `Introspect` on `/org/freedesktop/resolve1/link`.

Real cockpit-bridge encodes D-Bus `ay` byte arrays as base64 strings; the capability handles both base64 and JSON integer arrays transparently.

`dns flush` calls `FlushCaches()`. Unprivileged (polkit default allows it). Audited as operation `"dns-flush"`. Requires systemd-resolved.

`dns query <hostname>` calls `ResolveHostname(0, name, AF_UNSPEC, 0)`. Returns decoded IPv4/IPv6 addresses. NXDOMAIN maps to exit 4 (`not-found`). Requires systemd-resolved.

Canned fake bridge data: 3 resolve1 links (2 with DNS, 3 and 10 without), global DNS `192.168.1.1` + `fd00::1`, cache stats `(100, 500, 50)`. NM DnsManager fallback: mode `default`, one interface `enp1s0` with `192.168.1.1`. Tests depend on that canned state.

## Journal

The journal capability queries systemd journal entries by spawning `journalctl` on the target host via cockpit-bridge's stream channel. Read-only: no privilege escalation.

`fez journal` accepts journalctl-mirrored flags: `--unit` (repeatable), `--since`, `--until`, `--priority`, `--boot`, `--grep`, `--lines`, `--output-fields`. Default limit is 25 entries.

Discovery: `--list-boots` lists available boot IDs. `--list-fields` lists available journal field names for use with `--output-fields`.

Default fields per entry: `timestamp`, `hostname`, `identifier`, `pid`, `priority`, `message`. `--output-fields` adds fields to this set (never replaces).

Truncation: when more entries exist than `--lines`, the envelope includes `"truncated": true` and a hint suggesting `--since`, `--grep`, `--priority`, or increased `--lines`.

Plain text output uses journalctl-style one-liner format. Extra fields from `--output-fields` appear in brackets after the message.

`--list-boots` returns `JournalBoots`. `--list-fields` returns `JournalFields`. Entry queries return `JournalEntries`. All three use standard `fez/v1` envelopes.

The fake bridge serves canned journal data: 6 entries across 2 units (sshd, chronyd), 3 priorities (info, warning, err), and 2 boots. Fake filtering supports `--unit`, `--priority`, `--boot`, `--grep`, `--since`, and `--lines`. Tests depend on that canned state.

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

## Storage

The storage capability reads UDisks2 (`org.freedesktop.UDisks2`) over the bridge. Read-only: no mutations, no privilege escalation.

Three actions:

- `storage list` — block device inventory with filesystem type, label, UUID, size, and mount point.
- `storage show <device>` — full detail for one block device: partition info, partition table type, drive model/serial, LUKS encryption status.
- `storage health [--drive <filter>]` — NVMe/SMART drive health: temperature, power-on hours, critical warnings, self-test status.

UDisks2 sends device paths as byte arrays (`ay`) with trailing NUL; the capability decodes these transparently. Mount points arrive as `aay` and are similarly decoded.

`storage show` accepts both full paths (`/dev/nvme0n1p1`) and short names (`nvme0n1p1`). UDisks2 interfaces (`Filesystem`, `Partition`, `PartitionTable`, `Encrypted`, `NVMe.Controller`) are optional per object; absent interfaces return `None` rather than erroring.

Canned fake bridge topology: one NVMe drive (`Samsung SSD 990 PRO 4TB`), whole disk `nvme0n1` with GPT, three partitions (EFI vfat, ext4 boot, LUKS), and a dm cleartext device. Tests depend on that canned state.
