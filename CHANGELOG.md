# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
