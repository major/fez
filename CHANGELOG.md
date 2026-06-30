# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.0](https://github.com/major/fez/compare/v0.6.0...v0.7.0) - 2026-06-30

### Added

- *(dns)* fall back to NM DnsManager when resolved is absent
- *(dns)* add integration tests and dependency-missing handling
- *(dns)* add capability descriptors and output schemas
- *(dns)* implement status, query, and flush capabilities
- *(dns)* add DnsAction CLI enum and stub dispatch
- *(fake-bridge)* add canned resolve1 handler

### Fixed

- *(security)* restrict audit file sink paths to /tmp/fez and /run/fez
- *(security)* enforce SSH host-key verification and disable password auth
- *(security)* validate CLI input before crossing trust boundaries
- *(firewall)* protect ssh port removals by default
- *(audit)* stop trusting env for audit identity
- *(transport)* validate env-selected mechanisms
- *(transport)* restrict bridge env override
- *(protocol)* cap frame allocation at 16 MB to prevent OOM
- *(safety)* prevent systemd path bypass of protected-unit check
- *(ssh)* prevent argument injection via target host string
- *(dns)* address CodeRabbit review findings
- *(dns)* handle base64-encoded byte arrays from real cockpit-bridge

### Other

- *(dns)* document dual-backend (resolve1 + NM fallback)
- *(dns)* add DNS capability to agent docs and architecture map

## [0.6.0](https://github.com/major/fez/compare/v0.5.0...v0.6.0) - 2026-06-26

### Added

- *(system)* add PCP metrics snapshot via metrics1 channel
- *(storage)* add read-only UDisks2 storage capability
- *(system)* add system overview capability

### Other

- Merge pull request #124 from major/renovate/actions-checkout-7.x
- *(renovate)* cap rust version at MSRV 1.92
- *(deps)* update taiki-e/install-action action to v2.82.0
- *(system)* improve metrics patch coverage
- Merge pull request #121 from major/feat/storage
- *(cli)* group help output into Subsystems and Agent Discovery
- *(cli)* remove MCP support
- *(cli)* remove completions subcommand

## [0.5.0](https://github.com/major/fez/compare/v0.4.0...v0.5.0) - 2026-06-18

### Fixed

- *(packages)* reject malformed dnf5 sessions
- *(error)* stop repeating remediation in DependencyMissing message
- drain bridge stderr

### Other

- *(services)* document split entry points
- *(services)* cover split branches
- *(services)* trim split plumbing
- *(services)* keep public module path
- *(services)* split capability module
- *(network)* simplify split module internals
- *(network)* split network capability module
- Merge pull request #109 from major/rename-capability-schema
- apply rustfmt ordering
- rename capability schema registry to schema
- ignore .worktrees directory
- *(packages)* split backend modules
- *(protocol)* replace manual variant helpers with Variant<T> derive structs
- fix coverage gaps and update codecov ignore path
- apply cargo fmt and fix clippy type_complexity warning
- *(capability)* extract output schemas into schemas submodule
- *(fake_bridge)* split into per-service modules
- *(firewall)* split god module into reads, mutations, and zone submodules
- *(error)* centralize error hints in FezError::hints()
- *(fake_bridge)* extract reply helpers and eliminate boilerplate
- *(capability)* move schema/flag metadata into data tables
- *(firewall)* split 241-line mutate into per-mutation helpers
- *(packages)* share plan kind and human summary across backends
- *(capabilities)* dedupe bridge bootstrap and service-unknown mapping
- report top 3 Rust code smells by affected lines
- *(firewall)* group audited mutation calls
- *(services)* cover follow log streaming
- *(services)* type unit/journal output via shared Variant<T>
- *(firewall)* type mutation output models
- *(firewall)* cover status output model states
- *(firewall)* type read output models
- *(packages)* type packagekit mutation planning
- *(packages)* type transaction plan parsing
- *(packages)* type read output models
- *(services)* type enablement call metadata
- *(firewall)* cover action classification
- *(firewall)* split read and mutation dispatch
- type service unit protocol parsing

## [0.4.0](https://github.com/major/fez/compare/v0.3.0...v0.4.0) - 2026-06-11

### Added

- *(mcp)* add expanded capability tools
- *(describe)* expose typed argument metadata
- *(packages)* fall back to PackageKit when dnf5daemon is absent
- *(packages)* PackageKit read and mutation operations
- *(packages)* add PackageKit backend skeleton + signal parsing
- *(protocol)* add dbus_call_collect for signal-driven PackageKit calls
- *(protocol)* add DbusSignal parse type for signal frames

### Fixed

- *(capability)* expose output schemas
- *(firewall)* map dependency and unsupported-API failures to actionable errors
- *(cli)* emit fez/v1 error envelopes for --json usage and discovery errors
- *(cli)* hide --dry-run/--force on read-only command help
- *(describe)* complete plain-text output and generalize --force help
- *(capability)* include required <UNIT> in service mutation examples
- *(e2e)* don't abort RHEL 10 provisioning when dnf5daemon-server is absent

### Other

- Merge remote-tracking branch 'origin/main' into agents-progressive-discovery
- Merge pull request #81 from major/issue-57-expanded-mcp-tools
- Merge pull request #77 from major/dedup-tier1
- Merge pull request #78 from major/fix-54-typed-describe
- Merge pull request #75 from major/packagekit-fallback
- document the PackageKit fallback backend and test knobs
- *(packages)* integration suite for the PackageKit fallback backend
- *(fake-bridge)* add signal-emitting PackageKit arm and scenario knobs
- Merge pull request #72 from major/fix-52-json-errors
- Merge pull request #69 from major/fix-62-63-describe

## [0.3.0](https://github.com/major/fez/compare/v0.2.0...v0.3.0) - 2026-06-11

### Added

- *(firewall)* read masquerade and add runtime toggle
- *(firewall)* add masquerade on|off CLI action
- *(firewall)* guard masquerade disable behind --force
- *(firewall)* register capability descriptors
- *(firewall)* read commands and runtime-only mutations
- *(firewall)* add CLI surface, module, and dispatch route
- *(firewall)* add pure protected-op guards in safety
- *(network)* add read-only NetworkManager capability
- *(protocol)* drive transparent superuser mechanism fallback
- *(protocol)* add internal-bus dbus access for cockpit.Superuser
- *(packages)* unify list payloads on the columnar table shape
- *(packages)* add dnf5daemon-backed package management capability ([#23](https://github.com/major/fez/pull/23))
- LLM-optimized JSON output ([#21](https://github.com/major/fez/pull/21))
- *(services)* default bare unit names to .service ([#19](https://github.com/major/fez/pull/19))

### Fixed

- *(e2e)* cap ssh probes so a stalled connection can't hang the job
- *(firewall)* call getZones on the zone interface and escalate the drift read
- *(protocol)* unwrap variant envelope when reading Superuser.Bridges
- *(protocol)* complete handshake on bridge init, not superuser-init-done
- *(protocol)* diagnose superuser escalation failures with exit 11
- *(packages)* variant-wrap dnf5daemon a{sv} option arguments
- *(cli)* reset SIGPIPE to SIG_DFL so piped output exits cleanly ([#22](https://github.com/major/fez/pull/22))

### Other

- *(e2e)* drop auto issue-filing, surface failures inline
- *(firewall)* split force guard cases, add remove-port coverage
- *(firewall)* document masquerade in registry and AGENTS.md
- *(firewall)* cover masquerade read, toggle, guard, escalation
- *(firewall)* model masquerade in the fake bridge
- *(e2e)* address review feedback on harness scripts
- *(e2e)* hoist LOG_DIR before re-exec guard
- *(e2e)* surface forensics inline, poweroff before destroy, ensure e2e label
- *(e2e)* fan out parallel per-os matrix orchestrator
- *(e2e)* add isolated per-os provisioning + capability job
- *(e2e)* add github issue dedupe and redaction helpers
- *(e2e)* add per-capability test functions for all four capabilities
- *(e2e)* silence SC2001 on per-line sed indent
- *(e2e)* add shared bash+jq assertion helpers
- *(e2e)* install full capability surface, branch sudoers by os
- *(e2e)* derive ssh_user and ami_name from locals
- *(e2e)* select RHEL 10 or Fedora AMI by var.os
- *(e2e)* add os/rhel terraform variables
- extract shared integration test support into tests/common
- *(firewall)* document capability, fake bridge, and env knobs
- *(firewall)* integration tests against fake bridge
- *(firewall)* add firewalld reply arm to fake bridge
- pin stateless invariant in AGENTS.md, ignore local specs/plans
- document transparent escalation env vars and fake-bridge surface
- *(escalation)* cover transparent mechanism fallback (cases 1-6)
- *(fake-bridge)* model cockpit.Superuser Bridges/Start surface
- Merge pull request #24 from major/fix/dnf5daemon-server-remediation
- *(agents)* note compact envelope JSON for test assertions
- use buildless CodeQL extraction for Rust
- add supply-chain, MSRV, and CodeQL gates ([#17](https://github.com/major/fez/pull/17))

## [0.2.0](https://github.com/major/fez/compare/v0.1.0...v0.2.0) - 2026-06-10

### Fixed

- eliminate panics and enforce missing_docs ([#13](https://github.com/major/fez/pull/13))
- *(deps)* update rust dependencies to v2 ([#11](https://github.com/major/fez/pull/11))

### Other

- *(deps)* update release-plz/action action to v0.5.130 ([#16](https://github.com/major/fez/pull/16))
- centralize host resolution and fix local/localhost label drift ([#15](https://github.com/major/fez/pull/15))
- *(deps)* update release-plz/action action to v0.5.129 ([#7](https://github.com/major/fez/pull/7))
- *(deps)* update taiki-e/install-action action to v2.81.9 ([#8](https://github.com/major/fez/pull/8))
- *(deps)* update taiki-e/upload-rust-binary-action action to v1.30.2 ([#9](https://github.com/major/fez/pull/9))
- *(deps)* update release-plz/action digest to 476794e ([#6](https://github.com/major/fez/pull/6))
- *(deps)* update codecov/codecov-action digest to fb8b358 ([#5](https://github.com/major/fez/pull/5))
- *(deps)* update actions/checkout digest to df4cb1c ([#4](https://github.com/major/fez/pull/4))
- *(audit)* replace 7-arg AuditRecord::new with context + Outcome
- add renovate config extending shared preset
- *(services)* replace unreachable! with total dispatch
- add dedicated test job for fast pass/fail feedback
- share enablement descriptors
- share no-bridge service setup
- share cli command setup
- share log line formatting
- share service enablement flow
- add CodeRabbit config inheriting shared baseline
- enable crates.io trusted publishing
