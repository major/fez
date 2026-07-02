# fez

`fez` is an **agent-native** management CLI for Fedora and RHEL. It gives both
LLM-driven agents and humans a uniform, structured, discoverable way to operate
a host — and, over SSH, a fleet — by reusing Cockpit's existing system plumbing
instead of scraping dozens of human-oriented CLIs.

## Who is this for?

- **LLM agents** that need machine-readable output, safe mutation guardrails,
  and a consistent discovery model (`fez describe`, `fez capabilities`).
- **Humans** who want fast system insight without remembering systemctl,
  firewall-cmd, dnf, and nmcli flags.

## Quick example

```bash
# Local host overview
fez system show --json

# Check a service on a remote host
fez --host web1 services status nginx.service --json

# Restart safely: inspect, dry-run, execute
fez describe services.restart --json
fez services restart nginx.service --dry-run --json
fez services restart nginx.service --json
```

## Start here

- **New to fez?** Go through [Getting Started](getting-started.md).
- **Building an LLM agent?** Read the [Agent Guide](agent-guide/index.md).
- **Source:** [github.com/major/fez](https://github.com/major/fez)
