# fez Docs Site — mkdocs + GitHub Pages

**Date:** 2026-07-01
**Status:** design approved

## Goal

Publish narrative docs for fez on GitHub Pages using mkdocs+material theme, built and deployed via GitHub Actions. The site serves both LLM agents and human users with a unified guide structure.

## Non-Goals

- No auto-generated command reference (deferred)
- No PR preview builds (main-only, fast CI)
- No changes to existing `docs/agents/` or `docs/superpowers/` directories
- No custom domain (use default `major.github.io/fez`)

## Site Structure

```
docs/                       ← mkdocs content root
  index.md                  ← what fez is, why it exists, quick example
  getting-started.md        ← install, first commands, --json, discovery tools
  agent-guide/
    index.md                ← the operating loop, mental model, reference map
    json-envelope.md        ← envelope anatomy, status codes, hints, parsing
    safety.md               ← guardrails, --dry-run, --force, protected units
    remote-hosts.md         ← SSH transport, --host, identities, ssh_config
    workflows.md            ← end-to-end LLM workflow examples
```

Source material for each page:

| Page | Source |
|------|--------|
| `index.md` | New, plus README excerpt |
| `getting-started.md` | New |
| `agent-guide/index.md` | `skills/fez/SKILL.md` (adapt tone) |
| `agent-guide/json-envelope.md` | `skills/fez/references/safety-and-errors.md` |
| `agent-guide/safety.md` | `skills/fez/references/safety-and-errors.md` |
| `agent-guide/remote-hosts.md` | `skills/fez/references/remote-hosts.md` |
| `agent-guide/workflows.md` | `skills/fez/references/workflow.md` |

## New Files

| File | Purpose |
|------|---------|
| `mkdocs.yml` | mkdocs config: material theme, explicit `nav:`, search, strict mode |
| `requirements.txt` | `mkdocs-material` (pinned) |
| `.github/workflows/docs.yml` | Build + deploy to `gh-pages` branch on push to `main` |
| `docs/index.md` | Landing page |
| `docs/getting-started.md` | Human quick-start |
| `docs/agent-guide/*.md` | Agent guide pages (adapt existing skill references) |

## mkdocs Configuration

- Theme: `material`
- Navigation: explicit `nav:` with sections matching the structure above
- `strict: true` (broken links and warnings fail the build)
- Search enabled (default with material)

## CI Workflow (`docs.yml`)

- **Trigger:** push to `main`, path-filtered to `docs/`, `skills/fez/`, `mkdocs.yml`, `requirements.txt`, `.github/workflows/docs.yml`
- **Steps:**
  1. Checkout (SHA-pinned, `persist-credentials: false`)
  2. Setup Python (SHA-pinned)
  3. `pip install -r requirements.txt`
  4. `mkdocs build --strict`
  5. Deploy `site/` to `gh-pages` branch with `peaceiris/actions-gh-pages` (SHA-pinned)
- **Permissions:** `contents: write` (for gh-pages push)

## Post-Deploy Setup

After the first deployment pushes the `gh-pages` branch, enable GitHub Pages in repo settings:

- Source: "Deploy from a branch"
- Branch: `gh-pages`, root directory (`/`)

## AGENTS.md Update

Add a reminder to the conventions section:

> When a feature changes the CLI surface, output format, safety behavior, or agent workflow, update the corresponding page in `docs/` and the skill references in `skills/fez/references/` in the same commit.

## Existing Files — Unchanged

- `skills/fez/SKILL.md` and `skills/fez/references/*.md` remain in place as the canonical agent skill
- `docs/agents/*.md` remain as contributor architecture docs (not part of the mkdocs site)
- `docs/superpowers/` remains for specs and plans
- README.md unchanged (still the landing for the GitHub repo page)
