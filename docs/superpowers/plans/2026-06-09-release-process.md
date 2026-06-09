# fez Release Process Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add release automation to fez: release-plz manages version bumps, changelog, tags, and GitHub releases; a CD workflow attaches prebuilt Linux binaries.

**Architecture:** Two GitHub Actions workflows plus one config file. `release-plz.yml` (two jobs: release-pr + release) runs on push to `main` and is authed with a PAT so the published-release event triggers downstream. `cd.yml` runs on `release: published` and matrix-builds the `fez` binary for `x86_64` and `aarch64` linux-gnu, uploading tarballs. `release-plz.toml` configures tag naming. No crates.io publishing.

**Tech Stack:** GitHub Actions, release-plz, release-plz/action, taiki-e/upload-rust-binary-action, dtolnay/rust-toolchain, yamllint (local validation).

---

## Reference: pinned action SHAs

Use these exact pins (resolved 2026-06-09). Each `uses:` line pins a commit SHA with a trailing version comment. Re-resolve only if a newer version is explicitly wanted.

| Action | Version | SHA |
| --- | --- | --- |
| `actions/checkout` | v6.0.3 | `9f698171ed81b15d1823a05fc7211befd50c8ae0` |
| `dtolnay/rust-toolchain` | master | `3c5f7ea28cd621ae0bf5283f0e981fb97b8a7af9` |
| `release-plz/action` | v0.5.99 | `5b353e755cfc29e2efff7365a831d69fef0aaea9` |
| `taiki-e/upload-rust-binary-action` | v1.9.1 | `57510bf386b3945b57963e73201cea60ca18dff4` |

Note on `dtolnay/rust-toolchain`: this action publishes per-channel tags (e.g. `stable`) rather than semver release tags, and its README recommends `@stable`. The SHA above is current `master`. Pin the SHA with a `# stable` comment.

## File structure

```text
.github/workflows/release-plz.yml   # version bump, changelog, tag, GitHub release (2 jobs)
.github/workflows/cd.yml            # build + attach Linux binaries on release (matrix)
release-plz.toml                    # release-plz config: tag name, GitHub release on
```

No change to `Cargo.toml` (`publish = false` stays).

Repo facts the plan depends on:
- Repo slug: `major-hayden/fez`. Default branch: `main`.
- Two bin targets exist: `fez` and `fez-fake-bridge`. Only `fez` ships. The CD build MUST name `bin: fez` so the fake bridge is never packaged.
- No `CHANGELOG.md` yet; release-plz creates it on first release PR.

---

## Task 1: release-plz config

**Files:**
- Create: `release-plz.toml`

- [ ] **Step 1: Write `release-plz.toml`**

```toml
[[package]]
name = "fez"
git_tag_name = "v{{ version }}"
git_release_enable = true
```

- [ ] **Step 2: Validate it parses as TOML**

Run: `python3 -c "import tomllib; tomllib.load(open('release-plz.toml','rb')); print('ok')"`
Expected: `ok`

- [ ] **Step 3: Commit**

```bash
git add release-plz.toml
git commit -m "ci: add release-plz config"
```

---

## Task 2: release-plz workflow

**Files:**
- Create: `.github/workflows/release-plz.yml`

- [ ] **Step 1: Create the workflow file**

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
        uses: actions/checkout@9f698171ed81b15d1823a05fc7211befd50c8ae0 # v6.0.3
        with:
          fetch-depth: 0
          persist-credentials: false
      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@3c5f7ea28cd621ae0bf5283f0e981fb97b8a7af9 # stable
      - name: Run release-plz release
        uses: release-plz/action@5b353e755cfc29e2efff7365a831d69fef0aaea9 # v0.5.99
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
        uses: actions/checkout@9f698171ed81b15d1823a05fc7211befd50c8ae0 # v6.0.3
        with:
          fetch-depth: 0
          persist-credentials: false
      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@3c5f7ea28cd621ae0bf5283f0e981fb97b8a7af9 # stable
      - name: Run release-plz release-pr
        uses: release-plz/action@5b353e755cfc29e2efff7365a831d69fef0aaea9 # v0.5.99
        with:
          command: release-pr
        env:
          GITHUB_TOKEN: ${{ secrets.RELEASE_PLZ_TOKEN }}
```

- [ ] **Step 2: Lint the YAML**

Run: `yamllint -d "{extends: default, rules: {line-length: disable, document-start: disable, truthy: {check-keys: false}}}" .github/workflows/release-plz.yml`
Expected: no output (exit 0). The `truthy` override silences the `on:` key warning; `document-start` override allows the missing `---`.

- [ ] **Step 3: Verify SHA pins and no crates.io token**

Run: `grep -c "uses:.*@[0-9a-f]\{40\}" .github/workflows/release-plz.yml`
Expected: `6` (three actions x two jobs, all pinned to 40-char SHAs).

Run: `grep -c "CARGO_REGISTRY_TOKEN" .github/workflows/release-plz.yml`
Expected: `0` (we never publish to crates.io).

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/release-plz.yml
git commit -m "ci: add release-plz workflow"
```

---

## Task 3: CD binary-build workflow

**Files:**
- Create: `.github/workflows/cd.yml`

- [ ] **Step 1: Create the workflow file**

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
        uses: actions/checkout@9f698171ed81b15d1823a05fc7211befd50c8ae0 # v6.0.3
        with:
          persist-credentials: false
      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@3c5f7ea28cd621ae0bf5283f0e981fb97b8a7af9 # stable
        with:
          targets: ${{ matrix.target }}
      - name: Build and upload fez binary
        uses: taiki-e/upload-rust-binary-action@57510bf386b3945b57963e73201cea60ca18dff4 # v1.9.1
        with:
          bin: fez
          target: ${{ matrix.target }}
          archive: fez-$tag-$target
          token: ${{ secrets.GITHUB_TOKEN }}
```

- [ ] **Step 2: Lint the YAML**

Run: `yamllint -d "{extends: default, rules: {line-length: disable, document-start: disable, truthy: {check-keys: false}}}" .github/workflows/cd.yml`
Expected: no output (exit 0).

- [ ] **Step 3: Verify the bin guard and owner guard**

Run: `grep -c "bin: fez" .github/workflows/cd.yml`
Expected: `1` (only `fez` ships, never `fez-fake-bridge`).

Run: `grep -c "github.repository == 'major-hayden/fez'" .github/workflows/cd.yml`
Expected: `1` (forks do not run the upload job).

- [ ] **Step 4: Verify SHA pins**

Run: `grep -c "uses:.*@[0-9a-f]\{40\}" .github/workflows/cd.yml`
Expected: `3` (checkout, rust-toolchain, upload-rust-binary-action).

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/cd.yml
git commit -m "ci: add CD workflow to attach Linux binaries on release"
```

---

## Task 4: Final cross-file verification

No new files. This task confirms the three pieces are internally consistent before the human does the one-time GitHub setup.

- [ ] **Step 1: Confirm all three files exist**

Run: `ls release-plz.toml .github/workflows/release-plz.yml .github/workflows/cd.yml`
Expected: all three paths listed, no "No such file" errors.

- [ ] **Step 2: Confirm tag name matches between config and intent**

Run: `grep "git_tag_name" release-plz.toml`
Expected: `git_tag_name = "v{{ version }}"` (produces `v0.1.0`-style tags).

- [ ] **Step 3: Confirm RELEASE_PLZ_TOKEN is used in release-plz.yml and NOT in cd.yml**

Run: `grep -c "RELEASE_PLZ_TOKEN" .github/workflows/release-plz.yml`
Expected: `2` (one per job).

Run: `grep -c "RELEASE_PLZ_TOKEN" .github/workflows/cd.yml`
Expected: `0` (cd.yml uses the default `GITHUB_TOKEN`, which is enough to upload assets).

- [ ] **Step 4: Confirm no em dashes or unlabeled fences leaked into committed files**

Run: `grep -rl $'\u2014' release-plz.toml .github/workflows/ || echo "no em dashes"`
Expected: `no em dashes`.

- [ ] **Step 5: Final commit (if any staged docs/plan changes remain)**

```bash
git status --short
```
Expected: clean tree, or only the plan/spec docs staged. Commit those if present:

```bash
git add docs/superpowers/
git commit -m "docs: add release process spec and plan"
```

---

## One-time manual GitHub setup (human, NOT automated by this plan)

These are done once in the GitHub UI after the workflows are pushed. They are listed here so the implementer reminds the human; they are not plan steps to execute locally.

1. **Create the PAT.** Fine-grained Personal Access Token scoped to `major-hayden/fez`:
   - `Contents: Read and write`
   - `Pull requests: Read and write`
   Store as repo secret `RELEASE_PLZ_TOKEN` (Settings -> Secrets and variables -> Actions).
2. **Allow Actions to manage PRs.** Settings -> Actions -> General -> Workflow permissions -> enable "Allow GitHub Actions to create and approve pull requests."
3. **First run.** On the next push to `main`, the `release-pr` job opens a "release vX.Y.Z" PR. Review and merge it to cut the first release; `cd.yml` then attaches binaries.

---

## Notes for the implementer

- **No live test possible locally.** release-plz only works against a real GitHub repo with the PAT configured. Verification in this plan is by linting and grep assertions, not by running the release. The true end-to-end test is the first real release PR after the manual setup.
- **`yamllint` config is inline** so no `.yamllint` file is added to the repo. If the repo later adopts a shared yamllint config, fold these rules in.
- **If a future `[[bin]]` should ship**, the `bin: fez` line in `cd.yml` must be revisited (it currently ships exactly one binary by name).
