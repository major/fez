# Design: fez release process (GitHub Actions + release-plz)

## Problem

fez has no release automation. Before pushing to GitHub we need a release
process that bumps the version, maintains a changelog, tags the release,
publishes a GitHub release, and attaches prebuilt Linux binaries.

fez is a Linux-only Fedora/RHEL CLI that drives `cockpit-bridge`. It is not
published to crates.io (`Cargo.toml` keeps `publish = false`), so the pipeline
is GitHub-release-centric, not registry-centric.

## Decisions

These were settled during brainstorming and are fixed for this spec:

- **Distribution:** GitHub release only. No crates.io publish. `publish = false`
  stays in `Cargo.toml`.
- **Binaries:** build and attach Linux `x86_64-unknown-linux-gnu` and
  `aarch64-unknown-linux-gnu` tarballs to each release. No musl, no macOS,
  no Windows, no RPM (RPM stays a separate concern in `packaging/`).
- **Topology:** Approach A. Two workflows. `release-plz.yml` handles the
  release; a separate `cd.yml` builds binaries on the published-release event.
- **Trigger token:** a fine-grained Personal Access Token stored as the
  `RELEASE_PLZ_TOKEN` secret. release-plz uses it so the `release: published`
  event actually fires `cd.yml` (the default `GITHUB_TOKEN` does not trigger
  downstream workflows).
- **Scope:** release process only. No CI workflow (fmt/clippy/test) in this
  spec; that is handled separately.
- **Repo slug:** `major-hayden/fez` (from `Cargo.toml` `repository`).
- **Default branch:** `main`.

## Repository facts that shape the design

- Single crate, not a workspace.
- Two binary targets: `fez` (the real CLI) and `fez-fake-bridge` (a test
  fixture in `src/bin/fake_bridge.rs`). **Only `fez` is shipped.** The CD build
  must name the binary explicitly so the fake bridge is never packaged.
- No `CHANGELOG.md` yet. release-plz creates and maintains it.

## Files added

```text
.github/workflows/release-plz.yml   # version bump, changelog, tag, GitHub release
.github/workflows/cd.yml            # build + attach Linux binaries on release
release-plz.toml                    # release-plz config (tag name, no crates.io)
```

No change to `Cargo.toml`.

## Workflow 1: `.github/workflows/release-plz.yml`

Based on the official two-job quickstart. Triggers on push to `main`. One job
opens/updates the release PR, the other publishes the release once that PR
merges. The PAT is what makes the published-release event trigger `cd.yml`.

```yaml
name: release-plz

on:
  push:
    branches:
      - main

permissions:
  contents: read

jobs:
  release-plz-release:
    name: release-plz release
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - name: Checkout repository
        uses: actions/checkout@v6  # pin to SHA at implementation time
        with:
          fetch-depth: 0
          persist-credentials: false
      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable  # pin to SHA
      - name: Run release-plz release
        uses: release-plz/action@v0.5  # pin to SHA, keep version comment
        with:
          command: release
        env:
          GITHUB_TOKEN: ${{ secrets.RELEASE_PLZ_TOKEN }}

  release-plz-pr:
    name: release-plz PR
    runs-on: ubuntu-latest
    permissions:
      contents: write
      pull-requests: write
    concurrency:
      group: release-plz-${{ github.ref }}
      cancel-in-progress: false
    steps:
      - name: Checkout repository
        uses: actions/checkout@v6  # pin to SHA
        with:
          fetch-depth: 0
          persist-credentials: false
      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable  # pin to SHA
      - name: Run release-plz release-pr
        uses: release-plz/action@v0.5  # pin to SHA
        with:
          command: release-pr
        env:
          GITHUB_TOKEN: ${{ secrets.RELEASE_PLZ_TOKEN }}
```

Notes:

- No `CARGO_REGISTRY_TOKEN` env var anywhere: we never publish to crates.io.
- Top-level `permissions: contents: read` is the floor; each job widens only
  what it needs.
- `persist-credentials: false` keeps the checkout token out of the git config;
  release-plz authenticates via `GITHUB_TOKEN` env instead.
- Every third-party action is pinned to a commit SHA with a trailing version
  comment. The `@v6` / `@v0.5` shown here are placeholders for readability; the
  implementation replaces them with SHAs.

## Workflow 2: `.github/workflows/cd.yml`

Triggers when release-plz publishes a GitHub release. Builds the `fez` binary
(only `fez`, never `fez-fake-bridge`) for two Linux targets and uploads a
`.tar.gz` per target to the release.

```yaml
name: cd

on:
  release:
    types: [published]

permissions:
  contents: read

jobs:
  upload-binaries:
    name: ${{ matrix.target }}
    runs-on: ubuntu-latest
    if: github.repository == 'major-hayden/fez'
    permissions:
      contents: write
    strategy:
      fail-fast: false
      matrix:
        include:
          - target: x86_64-unknown-linux-gnu
          - target: aarch64-unknown-linux-gnu
    steps:
      - name: Checkout repository
        uses: actions/checkout@v6  # pin to SHA
        with:
          persist-credentials: false
      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable  # pin to SHA
        with:
          targets: ${{ matrix.target }}
      - name: Build and upload fez binary
        uses: taiki-e/upload-rust-binary-action@v1  # pin to SHA
        with:
          bin: fez
          target: ${{ matrix.target }}
          archive: fez-$tag-$target
          token: ${{ secrets.GITHUB_TOKEN }}
```

Notes:

- `bin: fez` is mandatory. Without it the action would try to package both
  binary targets, including the `fez-fake-bridge` test fixture.
- `if: github.repository == 'major-hayden/fez'` stops forks from running the
  upload job.
- The default `GITHUB_TOKEN` is sufficient here: this workflow only *uploads*
  assets to an existing release, it does not need to trigger anything else.
- `upload-rust-binary-action` handles aarch64 cross-compilation internally
  (installs the linker/cross tooling), so no extra cross setup is needed.
- Archive name `fez-$tag-$target` yields e.g.
  `fez-v0.1.0-x86_64-unknown-linux-gnu.tar.gz`.

## Config: `release-plz.toml`

Single crate, so the config is minimal. Clean `vX.Y.Z` tags are important
because nothing else keys off the tag, but they read better and match common
convention.

```toml
[[package]]
name = "fez"
git_tag_name = "v{{ version }}"
git_release_enable = true
```

`git_release_enable = true` is the default; it is stated explicitly so the
intent (release-plz owns GitHub releases) is obvious to a future reader.
Changelog generation is on by default and writes `CHANGELOG.md` in the keep-a-
changelog format; no extra config needed.

## One-time manual setup

These steps are done once by a human in the GitHub UI. They are NOT automated
by this spec.

1. **Create the PAT.** A fine-grained Personal Access Token scoped to the
   `major-hayden/fez` repository with:
   - `Contents: Read and write`
   - `Pull requests: Read and write`
   Store it as the repository secret `RELEASE_PLZ_TOKEN`
   (Settings -> Secrets and variables -> Actions).
2. **Allow Actions to manage PRs.** Settings -> Actions -> General ->
   Workflow permissions -> enable "Allow GitHub Actions to create and approve
   pull requests." Required for the `release-pr` job to open its PR.
3. **Pin action SHAs.** At implementation time, resolve and pin the current
   commit SHA for each third-party action (`actions/checkout`,
   `dtolnay/rust-toolchain`, `release-plz/action`,
   `taiki-e/upload-rust-binary-action`), each with a `# vX.Y.Z` comment.

## Release flow (end to end)

1. Commits land on `main` using Conventional Commit messages.
2. `release-plz.yml` runs. `release-pr` opens/updates a "release vX.Y.Z" PR with
   the version bump and changelog.
3. A human reviews and merges that PR.
4. The merge pushes to `main`, `release-plz.yml` runs again, and `release`
   tags `vX.Y.Z` and publishes the GitHub release (authored via the PAT).
5. The `release: published` event fires `cd.yml`, which builds `fez` for both
   Linux targets and attaches the tarballs to the release.

## Out of scope

- CI workflow (fmt / clippy / test). Handled separately; release-plz assumes a
  green main.
- crates.io publishing. `publish = false` stays.
- musl / macOS / Windows binaries.
- RPM build and `packaging/` automation.
- A machine user / GitHub App for release authorship (PAT is authored by the
  token owner; can migrate to an App later if desired).

## Risks and tradeoffs

- **PAT lifecycle.** Fine-grained PATs expire. When it expires, the release PR
  silently stops being created. Mitigation: set a calendar reminder, or migrate
  to a GitHub App token later.
- **PAT scope is account-wide identity.** Releases are authored as the token
  owner. Acceptable for a single-maintainer project; revisit if more
  maintainers join.
- **aarch64 cross build.** Relies on `upload-rust-binary-action`'s built-in
  cross support. If fez later grows a C dependency that needs an aarch64
  sysroot, this job may need `cross` or a native aarch64 runner.
- **Two-binary footgun.** If a future `[[bin]]` is added that *should* ship,
  the `bin: fez` line must be revisited. Documented here so it is not a
  surprise.
