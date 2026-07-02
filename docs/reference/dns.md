# DNS

Inspect DNS resolver configuration, flush the cache, and resolve hostnames.

**Backend:** systemd-resolved (`org.freedesktop.resolve1`) is the primary backend
(default on Fedora 41+). Fez automatically falls back to NetworkManager's
DnsManager interface when resolved is absent (e.g. RHEL 10). The fallback has
a reduced feature set — flush and query are unavailable.

## Read

### `dns status`

Show DNS resolver configuration and cache statistics.

```
fez dns status --json
fez dns status --all
```

| Flag | Purpose |
|------|---------|
| `--all` | Include links with no DNS servers configured |

Output kind: `DnsStatus`. On resolved, includes:

- Global DNS servers, DNSSEC mode, DNS-over-TLS setting
- LLMNR and multicast DNS configuration
- resolv.conf mode (uplink, static, etc.)
- Cache statistics (size, hits, misses)
- Per-link DNS detail (DNS servers, search domains, DNSSEC per-link setting)

On the NetworkManager fallback, the output carries `"backend":"networkmanager"`
and a hint noting the reduced feature set. NM output includes the DNS mode,
resolv.conf manager, and per-interface DNS servers.

### `dns query`

Resolve a hostname to addresses. Requires systemd-resolved.

```
fez dns query example.com --json
fez --host web1 dns query internal.corp --json
```

Output kind: `DnsQuery`. Returns the canonical name and all resolved IPv4 and
IPv6 addresses. NXDOMAIN (name not found) returns exit 4 (`not-found`).

Uses `ResolveHostname(0, name, AF_UNSPEC, 0)` — resolves all address families
without name service switching restrictions.

## Write

### `dns flush`

Clear the systemd-resolved DNS cache. Requires systemd-resolved.

```
fez dns flush --json
```

Output kind: `DnsFlush`. Unprivileged: the default polkit policy allows
`FlushCaches` without authentication. Audit-logged as a mutation because it
destroys cache state.

## Backend Selection

Fez probes resolved first. If `org.freedesktop.resolve1` is absent on D-Bus,
it falls back to `org.freedesktop.NetworkManager.DnsManager` for `dns status`.
The fallback is automatic; there is no flag to select a backend.

When resolved is absent and `dns flush` or `dns query` are called, fez returns
exit 9 (`dependency-missing`) with remediation naming `systemd-resolved`.
