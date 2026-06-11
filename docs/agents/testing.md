# Testing Notes For Agents

Read this before changing tests, fake bridge behavior, JSON assertions, test-only env vars, or E2E infrastructure.

## Integration Scaffolding

- Shared integration scaffolding lives in `tests/common/mod.rs` and is included per suite with `mod common;`.
- `fez_fake()` wires tests to the fake bridge through `FEZ_BRIDGE=env!("CARGO_BIN_EXE_fez-fake-bridge")`.
- `fez_fake_quiet()` is the same helper with `FEZ_AUDIT=off`; `tests/firewall.rs` imports it as `fez_fake`.
- `fez_plain()` is for CLI-surface tests that do not need a bridge.
- `fez_fake_pk()` is `fez_fake_quiet()` plus `FEZ_FAKE_NO_DNF5`, used by `tests/packages_pk.rs`.
- `PKG_COLUMNS` is the `PackageList` table header constant used by package tests.
- `AuditLog::new(label)`, `env_value()`, and `records()` provide an RAII audit sink for services and packages tests.
- Keep suite-specific helpers local, such as `fez_without_bridge`, `fake_transport`, and `fez_mcp`.
- `tests/common/` is a module directory, not its own integration test binary.

## Fake Bridge

Integration tests use `src/bin/fake_bridge.rs`, built as `fez-fake-bridge`, instead of a real `cockpit-bridge`.

General fake bridge contracts:

- Reports `chronyd` inactive and `sshd` active.
- Models deferred cockpit escalation over `cockpit.Superuser`; see `docs/agents/capabilities.md`.
- Rejects unwrapped `a{sv}` option dictionary values through `reject_unwrapped_options`.
- DNF option dictionaries must be built through `options()` and `variant()` in `src/capabilities/packages.rs`.
- NetworkManager fake dispatch is object-path-sensitive because generic `Get` and `GetAll` methods are reused across object types.
- Firewalld fake dispatch is object-path- and interface-sensitive; methods called on the wrong interface should reproduce real `UnknownMethod` failures.

PackageKit fake bridge contracts:

- `CreateTransaction` on `/org/freedesktop/PackageKit` returns a canned transaction object path.
- Calls on `/org/freedesktop/PackageKit/Transaction/1` are routed before the dnf arm.
- Transaction methods emit `Package`, `RepoDetail`, `ErrorCode`, and `Finished` signal frames collected through `dbus_call_collect`.

Firewalld fake bridge contracts:

- Runtime `public` includes `ssh`, `dhcpv6-client`, `9090/tcp`, and masquerade.
- Permanent `public` lacks `9090/tcp` and masquerade, so status reports drift by default.
- Permanent config reads require a privileged channel unless a test knob explicitly models partial read denial.

## Fake Environment Knobs

| Env var | Meaning |
| --- | --- |
| `FEZ_BRIDGE` | Bridge binary path used by tests. |
| `FEZ_AUDIT` | Audit sink; tests commonly use `off` or `file:/path`. |
| `FEZ_FAKE_PLAN` | Selects dnf5daemon `Goal.resolve` plan: `install`, `small`, `protected`, or `cascade`. |
| `FEZ_FAKE_PACKAGE_COUNT` | Makes fake `Rpm.list` return a generated large package set for large-response hint tests. |
| `FEZ_FAKE_NO_DNF5` | Simulates absent dnf5daemon. |
| `FEZ_FAKE_NO_PACKAGEKIT` | Simulates absent PackageKit so the both-backends-absent exit-9 path is testable. |
| `FEZ_FAKE_PK_PLAN` | `protected` adds `systemd` to the PackageKit removal plan so the dangerous-transaction guardrail fires. |
| `FEZ_FAKE_PK_ERROR` | `notauth` injects a PackageKit `NOT_AUTHORIZED` error, mapped to exit 11. |
| `FEZ_FAKE_BRIDGES` | Ordered `cockpit.Superuser` mechanism list like `sudo:ok` or `sudo:err,polkit:ok`; unset defaults to `sudo:ok`, empty means no mechanism. |
| `FEZ_FAKE_DENY_PRIVILEGED` | Denies privileged channel after successful escalation. |
| `FEZ_FAKE_NO_FIREWALLD` | Simulates absent firewalld. |
| `FEZ_FAKE_FIREWALLD_UNREACHABLE` | Closes the FirewallD1 channel as `not-found`, exercising dependency mapping. |
| `FEZ_FAKE_NO_MASQUERADE` | Returns `UnknownMethod` for `getMasquerade`, exercising unsupported-API mapping. |
| `FEZ_FAKE_PANIC` | Starts fake firewalld in panic mode. |
| `FEZ_FAKE_PORT_REMOVED` | Drops `9090/tcp` from runtime `public`, modeling post-remove-port state. |
| `FEZ_FAKE_CONFIG_INFO_DENIED` | Denies permanent firewalld `config.info` reads after escalation; `firewall status` should return partial runtime status with hints, and `firewall reload` should require `--force`. |
| `FEZ_FAKE_CONFIG_UNKNOWN_METHOD` | Returns `UnknownMethod` from the permanent firewalld config root, used to prove only `config.info` denial is converted into the reload guard. |

## Compact JSON Assertions

- Envelope JSON is compact: `Envelope::to_json_string` uses `serde_json::to_string`.
- Assert `--json` output with compact substrings such as `"kind":"ServiceList"`.
- Do not assert pretty-printed substrings such as `"kind": "ServiceList"`; those can pass against stale local builds and fail in CI.

## Package Test Notes

- `tests/packages.rs` drives the fake over `org.rpm.dnf.v0`.
- The fake `Rpm.list` returns a fixed multi-repo set: `bash`, `htop`, and `nginx` from `fedora`, plus `vim-enhanced` from `updates`.
- Package tests cover `--repo`, `--name`, `--limit`, and `--offset`; `FEZ_FAKE_PACKAGE_COUNT` generates large result sets for large-response hint tests.
- `FEZ_FAKE_NO_DNF5` makes dnf5daemon absent and exercises dependency/fallback paths.
- `tests/packages_pk.rs` drives PackageKit fallback with `FEZ_FAKE_NO_DNF5` set through `fez_fake_pk()`.
- A no-escalation host (`FEZ_FAKE_BRIDGES=""`) fails PackageKit mutations with exit 11.

## Network Test Notes

- `tests/network.rs` drives the fake over `org.freedesktop.NetworkManager`.
- Canned devices are `enp1s0`, `enp2s0`, `lo`, and `veth0`.
- Every `a{sv}`/variant value is variant-wrapped like the systemd `GetAll` arm.
- `network list` hides unmanaged virtual interfaces unless `--all`.

## Firewall Test Notes

- `tests/firewall.rs` drives the fake over `org.fedoraproject.FirewallD1`.
- Runtime reads are unprivileged; permanent config reads and mutations require escalation.
- An empty `FEZ_FAKE_BRIDGES` value yields exit 11 for mutating calls and for `status` because status escalates for permanent drift reads.
- `FEZ_FAKE_CONFIG_INFO_DENIED=1` models a real-host partial-read case after escalation; keep empty `FEZ_FAKE_BRIDGES` for the separate no-escalation case.
- `firewall reload` reads permanent config before mutating so it can refuse drift-discarding reloads without `--force`. If `FEZ_FAKE_CONFIG_INFO_DENIED=1` makes that read unavailable, reload treats drift as unknown-but-unsafe: without `--force` it exits 8, and with `--force` it proceeds.
- `FEZ_FAKE_CONFIG_UNKNOWN_METHOD=1` keeps unrelated permanent-config errors passing through, so tests prove the reload guard only catches real-host `config.info` authorization denial.
- Protected firewall operations return exit 8 unless `--force` is present.

## E2E Matrix

`test/e2e/run.sh` provisions real cloud hosts with Terraform, installs `cockpit-bridge` plus backends, and exercises the real transport. It is expensive and destructive.

- Default OS matrix is Fedora plus RHEL 10; override with `FEZ_E2E_OS="fedora rhel10"`.
- The harness performs one Terraform apply over `var.oses`; Terraform parallelizes per-OS instances through `for_each`.
- Readiness waits on `/var/lib/fez-e2e-ready` with a hard timeout (`var.ready_timeout_seconds`, default 300).
- A failed host taints only its own instance. The runner tolerates apply failure and derives survivors from Terraform outputs.
- Capability tests loop over `READY_OSES`, recording `os cap status` triples in a result file for the matrix table.
- Teardown is a single `EXIT` trap that destroys the shared Terraform stack, including tainted hosts.
- Failures are not auto-filed as issues; forensics are printed inline to the matrix log.
- Runs write `test/e2e/logs/matrix-<ts>.log` and update `test/e2e/logs/last-run.log`; inspect the log on failure.
- SSH behavior is pinned with `FEZ_SSH_CONFIG` because OpenSSH ignores `$HOME/.ssh/config` non-interactively.
- AWS credential preflight uses `aws sts get-caller-identity` and exits 2 when credentials are unavailable.
- E2E provisioning hard-requires `cockpit-bridge cockpit-system firewalld`.
- `dnf5daemon-server` is installed only for OSes where it exists. RHEL 10 lacks it; current package capability E2E treats exit 9 as `skip`, though the PackageKit fallback should be revisited in follow-up work.
