# Storage

Inspect block devices, partitions, filesystems, and drive health.

**Backend:** UDisks2 over D-Bus. Requires `udisks2` on the target.
Read-only; no privilege escalation.

## Commands

### `storage list`

List block devices with filesystem type, label, UUID, size, and mount point.

```
fez storage list --json
fez --host web1 storage list
```

Output kind: `StorageDeviceList`. Includes all block devices UDisks2 knows
about: whole disks, partitions, LVM volumes, dm-crypt/LUKS devices, and loop
devices.

### `storage show`

Show the full detail for one block device.

```
fez storage show /dev/nvme0n1p1 --json
fez storage show sda1 --json
```

Accepts both full device paths (`/dev/nvme0n1p1`) and short names (`nvme0n1p1`,
`sda1`).

Output kind: `StorageDeviceDetail`. Includes:

- Size and block size
- Filesystem type, label, UUID, and usage (used/free)
- Mount points
- Partition info (number, type UUID, offset, size, flags)
- Partition table type (GPT, MBR)
- Drive model, revision, and serial number
- LUKS encryption status (cleartext device if unlocked)

UDisks2 interfaces (`Filesystem`, `Partition`, `PartitionTable`, `Encrypted`,
`NVMe.Controller`) are optional per device. Absent interfaces return `null`
rather than erroring.

### `storage health`

Show NVMe/SMART drive health data.

```
fez storage health --json
fez storage health --drive Samsung --json
```

| Flag | Purpose |
|------|---------|
| `--drive <filter>` | Filter by drive model, serial, or path substring |

Output kind: `StorageHealth`. Shows only drives with NVMe controller data:

- Temperature (composite sensor in Kelvin and Celsius)
- Power-on hours
- Critical warnings (available spare, reliability degraded, read-only mode)
- SMART self-test status (passed/failed, percentage remaining)

The `--drive` filter matches against model, serial, or device path substring.
