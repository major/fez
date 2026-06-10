# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
