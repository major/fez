# Output Kinds

Every `fez/v1` success envelope carries a `kind` field identifying the response
shape. This page lists every possible kind, which commands produce it, and what
data to expect. For full JSON Schema, use `fez describe <id> --json`.

## Agent Discovery

| Kind | Produced by | Data |
|------|-------------|------|
| `AgentGuide` | `fez guide --json` | orientation command, discovery steps, envelope fields, global flags, exit codes, env vars |
| `CapabilityList` | `fez capabilities --json` | `capabilities` array of dotted capability ids |
| `CapabilityDescriptor` | `fez describe <id> --json` | full descriptor: `id`, `summary`, `long`, `privileged`, `output_kind`, `inputs`, `flags`, `flag_schema`, `examples`, `output` (with JSON Schema) |

## Services

| Kind | Produced by | Data |
|------|-------------|------|
| `ServiceList` | `services list` | table: `name`, `description`, `load_state`, `active_state`, `sub_state` |
| `ServiceStatus` | `services status <unit>` | object: `id`, `description`, `load_state`, `active_state`, `sub_state`, `unit_file_state` |
| `LogEntries` | `services logs <unit>` | object: `unit` (string), `entries` array (`timestamp`, `priority`, `identifier`, `message`, `pid`) |
| `ServiceMutation` | `services start/stop/restart/reload` | object: `operation`, `unit`, `host`, `job` |
| `ServiceEnablement` | `services enable/disable` | object: `operation`, `unit`, `host`, `now`, `changes` |
| `DryRun` | any service mutation with `--dry-run` | object: `operation`, `unit`, `host`, `privileged`, `command` (the full CLI invocation that would run) |

## Packages

| Kind | Produced by | Data |
|------|-------------|------|
| `PackageList` | `packages list` | table (`name`, `evr`, `arch`, `repo_id`, `install_size`, `summary`) + pagination: `total`, `returned`, `limit`, `offset`, `next_offset` + `scope`, `repos`, `name`, `backend` |
| `PackageInfo` | `packages info <spec>` | object: `name`, `evr`, `arch`, `repo_id`, `install_size`, `summary`, `backend` |
| `PackageSearch` | `packages search <pattern>` | table (same columns as PackageList) + `pattern` string |
| `PackageUpdates` | `packages check-update` | table (same columns as PackageList) |
| `RepoList` | `packages repolist` | table: `id`, `name`, `enabled` + `backend` |
| `PackageMutation` | `packages install/remove/upgrade` | object: `operation`, `specs`, `dry_run`, `install`/`remove`/`upgrade`/`downgrade` arrays, `install_size_total`, `counts`, `backend` |
| `PackagePlan` | `packages install/remove/upgrade` with `--dry-run` | same as `PackageMutation` (the resolved transaction plan) |

## Network

| Kind | Produced by | Data |
|------|-------------|------|
| `NetworkDeviceList` | `network list` | table: `interface`, `type`, `state`, `ip4`, `ip6`, `mac` |
| `NetworkDeviceDetail` | `network show <device>` | object: `interface`, `type`, `state`, `mac`, `mtu`, `ipv4` (addresses, gateway, dns, domains), `ipv6` (addresses), `connection` (id, type, default), `dhcp4` |

## Firewall

| Kind | Produced by | Data |
|------|-------------|------|
| `FirewallStatus` | `firewall status` | object: `running`, `default_zone`, `panic_mode`, `masquerade`, `pending_changes`, `pending_changes_available` |
| `FirewallZoneList` | `firewall list` | table: `zone`, `default`, `services`, `ports`, `interfaces` |
| `FirewallZone` | `firewall show <zone>` | object: `zone`, `services`, `ports`, `interfaces`, `sources`, `masquerade` |
| `FirewallServiceCatalog` | `firewall services` | object: `services` array of service names |
| `FirewallChange` | `firewall add-service/remove-service/add-port/remove-port/set-default-zone/reload/panic/masquerade` | object: `operation`, `zone`, `change`, `persisted`, `panic_mode`, `timeout`, `masquerade` |
| `FirewallConfirm` | `firewall confirm` | object: `operation`, `persisted` (always true) |

## System

| Kind | Produced by | Data |
|------|-------------|------|
| `SystemOverview` | `system show` | object: `hostname`, `machine_id`, `boot_id`, `os`, `os_id`, `os_version_id`, `kernel`, `kernel_release`, `hardware_vendor`, `hardware_model`, `chassis`, `firmware_vendor`, `firmware_version`, `timezone`, `ntp_enabled`, `ntp_synchronized`, `time_utc`, `rtc_time_utc`, `os_release` (full parsed map) |
| `SystemMetrics` | `system metrics` | object: `cpu` (user, sys, iowait, idle), `memory` (used, buffers, cached, free), `load` (1, 5, 15 min), `disk` (read/write IOPS via rate), `network` (per-interface read/write bytes/sec via rate) |
| `SessionList` | `system sessions` | table: `id`, `user`, `seat`, `service`, `type`, `class`, `remote_host`, `remote_user`, `state` |
| `UserList` | `system users` | table: `uid`, `user` |
| `InhibitorList` | `system inhibitors` | table: `what` (inhibited action), `who` (process description), `why` (reason), `mode` (block/delay) |
| `BootEntryList` | `system boot-entries` | table: `entry` (filename) |
| `SubscriptionStatus` | `system subscription` | object: `uuid` (consumer UUID), `status` (entitlement status string), `installed_products` array, `system_purpose` object (role, usage, service_level) |
| `FirmwareDeviceList` | `system firmware list` | table: `name`, `vendor`, `version`, `updatable` |
| `FirmwareSecurityReport` | `system firmware security` | object: `hsi` (HSI level string), `attributes` array (name, result, required_hsi_level) |
| `FirmwareUpgradeList` | `system firmware upgrades` | table: `device` (name), `current` (version), `available` (version), `description` |
| `PowerAction` | `system reboot/poweroff/suspend` | object: `action`, `host`, `confirmed` (always true after --force) |

## Storage

| Kind | Produced by | Data |
|------|-------------|------|
| `StorageDeviceList` | `storage list` | table: `device` (path), `type`, `fs_type`, `label`, `uuid`, `size`, `mount_point` |
| `StorageDeviceDetail` | `storage show <device>` | object: device info (size, model, serial), filesystem (type, label, uuid, mount points, used/free), partition (number, type, flags), partition table (type), LUKS (cleartext device, hint) |
| `StorageHealth` | `storage health` | table: `drive` (model), `temperature` (kelvin), `power_on_hours`, `critical_warnings`, `self_test` |

## DNS

| Kind | Produced by | Data |
|------|-------------|------|
| `DnsStatus` | `dns status` | **resolved**: object with `global` (DNS servers, DNSSEC, DNS-over-TLS, LLMNR, MDNS, resolv.conf mode, cache stats) + `links` array (per-link DNS servers, domains, DNSSEC). **NM fallback**: object with `backend` ("networkmanager"), `mode`, `rc_manager`, `dns_servers`, `interfaces` |
| `DnsFlush` | `dns flush` | object: `flushed` (boolean) |
| `DnsQuery` | `dns query <hostname>` | object: `hostname`, `canonical` (resolved name), `addresses` array (`family`, `address`, `ifindex`) |

## Journal

| Kind | Produced by | Data |
|------|-------------|------|
| `JournalEntries` | `fez journal` | object: `entries` array (`timestamp`, `hostname`, `identifier`, `pid`, `priority`, `message` + any `--output-fields`), `lines` (count), `truncated` (boolean) |
| `JournalBoots` | `fez journal --list-boots` | object: `boots` array (`id`, `boot_id`, `first` timestamp, `last` timestamp) |
| `JournalFields` | `fez journal --list-fields` | object: `fields` array of field name strings |
