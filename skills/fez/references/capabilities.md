# Fez Capability Cookbook

Run `fez capabilities` for the live list and `fez describe <id> --json` for exact inputs, flags, examples, output kind, and schema.

## System

```bash
fez system show --json
fez system metrics --json
fez system sessions --json
fez system users --json
fez system inhibitors --json
fez system boot-entries --json
fez system subscription --json
fez system firmware list --json
fez system firmware security --json
fez system firmware upgrades --json
```

Power actions are protected mutations. Inspect descriptors and user intent first:

```bash
fez describe system.reboot --json
fez system reboot --force --json
```

## Services

```bash
fez services list --json
fez services list --state failed --json
fez services status sshd.service --json
fez services logs sshd.service --lines 100 --json
```

Mutations include `start`, `stop`, `restart`, `reload`, `enable`, and `disable`:

```bash
fez describe services.restart --json
fez services restart nginx.service --dry-run --json
fez services restart nginx.service --json
```

## Packages

```bash
fez packages search nginx --json
fez packages info nginx --json
fez packages list --limit 100 --json
fez packages check-update --json
fez packages repolist --json
```

Mutations include `install`, `remove`, and `upgrade`:

```bash
fez describe packages.install --json
fez packages install nginx --dry-run --json
fez packages install nginx --json
```

Package removals can trigger dangerous-transaction guardrails. Read the descriptor and dry-run output before removing packages.

## Network

```bash
fez network list --json
fez network list --all --json
fez network show enp1s0 --json
```

Network commands are read-only. Use them for interface state, IP configuration, active connections, and DHCP details.

## Firewall

```bash
fez firewall status --json
fez firewall list --json
fez firewall show public --json
fez firewall services --json
```

Runtime mutations include adding and removing services or ports, setting the default zone, reload, confirm, panic mode, and masquerade:

```bash
fez describe firewall.add-service --json
fez firewall add-service public https --dry-run --json
fez firewall add-service public https --json
fez firewall confirm --json
```

Firewall guardrails protect lockout-prone operations. Use `--force` only with explicit user intent and descriptor-confirmed risk.

## Storage

```bash
fez storage list --json
fez storage show nvme0n1p1 --json
fez storage health --json
fez storage health --drive nvme0 --json
```

Storage commands are read-only and use UDisks2 data.

## DNS

```bash
fez dns status --json
fez dns query example.com --json
fez dns flush --json
```

`dns status` can use systemd-resolved or NetworkManager fallback. If the fallback lacks a feature, expect `dependency-missing` with remediation hints.
