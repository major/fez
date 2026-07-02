# Network

Inspect network devices, addresses, routes, and DHCP configuration.

**Backend:** NetworkManager over D-Bus. No extra target packages required.
Read-only; no privilege escalation.

## Commands

### `network list`

List network devices with type, state, primary IP, and MAC address.

```
fez network list --json
fez network list --all
```

| Flag | Purpose |
|------|---------|
| `--all` | Include every device, including unmanaged virtual interfaces (veth, etc.) |

Output kind: `NetworkDeviceList`. By default, unmanaged virtual interfaces are
hidden. Each entry shows the interface name, device type (ethernet, loopback,
veth, bond, etc.), operational state, and primary IPv4 and IPv6 addresses.

### `network show`

Show full detail for one device.

```
fez network show enp1s0 --json
```

Output kind: `NetworkDeviceDetail`. Includes:

- IPv4 and IPv6 addresses with prefix length
- Gateway and DNS servers
- Search domains
- Routes
- MAC address and MTU
- Active connection profile (id, UUID)
- DHCP lease (server, lease time, renewal time)

Look up devices by interface name (e.g. `enp1s0`, `lo`, `wlp3s0`). Unknown
devices return exit 4 (`not-found`).

## Unmanaged Virtual Interfaces

NetworkManager classifies container veth, bridge, and virtual interfaces as
unmanaged. These are hidden from `network list` by default. Use `--all` to
include them. `network show` works on any interface regardless of management
state.
