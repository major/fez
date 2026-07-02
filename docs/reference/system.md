# System

Host overview, performance metrics, user sessions, boot entries, subscription
status, firmware, and power actions.

**Backend:** systemd (hostname1, timedate1), logind, RHSM, fwupd, PCP. Most
reads need no extra packages; some subcommands require target dependencies.

## Read

### `system show`

Gather a complete host overview in one call. Run this first when orienting to
a host.

```
fez system show --json
```

Output kind: `SystemOverview`. Includes:

- Hostname, machine ID, boot ID
- OS identity and version (from `/etc/os-release`)
- Kernel version and architecture
- Hardware vendor, model, and chassis type
- Firmware type (BIOS/UEFI)
- Virtualization (none, kvm, etc.)
- Timezone and NTP synchronization status

No extra target packages required — uses systemd's own `hostname1` and `timedate1`
interfaces present on every Fedora and RHEL system.

### `system metrics`

Collect a one-shot PCP performance snapshot.

```
fez system metrics --json
```

Output kind: `SystemMetrics`. Includes:

- CPU usage (user, system, iowait, idle)
- Memory utilisation (used, buffers, cached, free)
- Load averages (1, 5, and 15 minutes)
- Aggregate disk IOPS
- Per-interface network throughput (bytes/sec in and out)

Requires `pcp` and `python3-pcp` on the target. Uses two 1-second samples for
rate-derived counters (disk I/O and network throughput).

### `system sessions`

List active login sessions from systemd-logind.

```
fez system sessions --json
```

Output kind: `SessionList`. Shows session ID, username, type (tty, x11,
wayland), remote host if applicable, service, and state.

### `system users`

List users with active login sessions.

```
fez system users --json
```

Output kind: `UserList`. Shows UID and username.

### `system inhibitors`

List active shutdown and sleep inhibitor locks.

```
fez system inhibitors --json
```

Output kind: `InhibitorList`. Shows what is inhibited (shutdown, sleep, etc.),
which application holds the lock, the reason, and the lock mode (block or
delay).

### `system boot-entries`

List Boot Loader Specification (BLS) entries.

```
fez system boot-entries --json
```

Output kind: `BootEntryList`. Shows the filename of each entry from the
bootloader's BLS directory.

### `system subscription`

Show RHEL subscription status. RHEL only — absent on Fedora (exit 9).

```
fez system subscription --json
```

Output kind: `SubscriptionStatus`. Includes:

- Consumer UUID
- Entitlement status
- Installed products
- System purpose (role, usage, SLA)

Requires `subscription-manager` on the target — only available on RHEL.

### `system firmware`

Inspect firmware-updatable devices, security posture, and available upgrades
via fwupd. Requires `fwupd` on the target.

#### `system firmware list`

List all firmware-updatable devices.

```
fez system firmware list --json
```

Output kind: `FirmwareDeviceList`. Shows device name, vendor, current version,
and whether the device is updatable.

#### `system firmware security`

Show the Host Security ID (HSI) score and individual security attributes.

```
fez system firmware security --json
```

Output kind: `FirmwareSecurityReport`. HSI levels range from 0 (insecure) to
4 (hardened). Each attribute shows its result and required HSI level.

Common attributes: TPM presence, Secure Boot status, IOMMU protection, BIOS
write protection.

#### `system firmware upgrades`

List available firmware upgrades for all updatable devices.

```
fez system firmware upgrades --json
```

Output kind: `FirmwareUpgradeList`. Shows the device name, current version,
available version, and upgrade description. Empty when no upgrades are available.

## Write

All power actions are **privileged** (require cockpit escalation), audit-logged,
and require `--force` to confirm.

### `system reboot`

Reboot the host via systemd-logind. Checks `CanReboot` first.

```
fez system reboot --force
fez --host web1 system reboot --force --json
```

Returns exit 9 if the host does not support reboot via logind.

### `system poweroff`

Power off the host via systemd-logind. Checks `CanPowerOff` first.

```
fez system poweroff --force
```

### `system suspend`

Suspend the host via systemd-logind. Checks `CanSuspend` first.

```
fez system suspend --force
```

Returns exit 9 if suspend is not available (common on headless servers).
