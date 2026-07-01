# fez

An agent-native management CLI for Fedora/RHEL. `fez` gives LLM-driven agents a
uniform, structured, discoverable way to operate a host (and, over SSH, a fleet)
by reusing Cockpit's existing system plumbing instead of scraping dozens of
human-oriented CLIs.

`fez` drives `cockpit-bridge` over its framed JSON protocol: it spawns the
bridge, opens channels (`dbus-json3`, `stream`, privileged `superuser`), and
delegates privilege escalation to the bridge. Remote hosts are reached by
shelling out to the system OpenSSH client.

## Install

```bash
sudo dnf install fez
```

Requires `cockpit-bridge` (the substrate) and `openssh-clients` (remote
transport); both are pulled in automatically.

## Agent skill

Agents can install the fez skill directly from this repository:

```bash
npx skills add major/fez --skill fez
```

The skill teaches the safe operating loop for `fez`: orient with `fez guide` or `fez system show`, discover capabilities, inspect unfamiliar commands with `fez describe`, prefer `--json`, and treat mutations with dry-run and guardrail awareness.

## Usage

```bash
# Discover capabilities on demand (the git model: nothing preloaded).
fez capabilities
fez describe services.status --json

# System overview — the first thing an agent should run.
fez system show --json

# Read-only (no elevation).
fez services list --state failed
fez services status sshd.service --json
fez services logs sshd.service --lines 100 --json

# Mutations (privileged, guarded by the safety layer).
fez services restart chronyd.service --dry-run
fez services restart chronyd.service

# Target a remote host (ssh_config aliases work).
fez --host web1 services status nginx.service --json
```

Every command supports `--json` (the `fez/v1` envelope), `--host` (localhost by
default), `--ssh-identities-only` (opt in to OpenSSH `IdentitiesOnly=yes`), and
layered `--help`.

## License

Apache-2.0
