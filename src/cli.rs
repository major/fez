use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "fez",
    version,
    about = "Agent-native management CLI for Fedora/RHEL"
)]
/// Top-level parsed command line.
pub struct Cli {
    /// Target host (localhost when omitted). May be a host, user@host, or ssh_config alias.
    #[arg(long, global = true)]
    pub host: Option<String>,

    /// Emit the machine-readable fez/v1 JSON envelope.
    #[arg(long, global = true)]
    pub json: bool,

    /// Preview the action without connecting or mutating (no-op for reads).
    #[arg(long, global = true)]
    pub dry_run: bool,

    /// Override command-specific safety guardrails. See command help for exact risks.
    #[arg(long, global = true)]
    pub force: bool,

    /// The subcommand to run.
    #[command(subcommand)]
    pub command: TopCommand,
}

impl Cli {
    /// The host label for the response envelope and audit records.
    ///
    /// Resolves the global `--host` flag through the same normalization the
    /// transport applies, so the reported label never drifts from the host the
    /// bridge actually runs on. In particular `--host local` and an omitted
    /// `--host` both report `localhost`, matching [`crate::transport::from_host`].
    #[must_use]
    pub fn resolved_host(&self) -> String {
        crate::transport::from_host(self.host.as_deref()).host_label()
    }
}

/// The derived clap command tree before registry enrichment.
pub fn raw_command() -> clap::Command {
    <Cli as CommandFactory>::command()
}

/// The fully enriched clap command (registry long-about and examples injected).
pub fn command() -> clap::Command {
    crate::schema::help::inject(raw_command())
}

/// Whether the raw argv requested machine-readable output (`--json`).
///
/// Used to decide error rendering before clap has parsed successfully: a parse
/// error means we have no [`Cli`] to read `json` from, so we scan the raw args.
/// `--json` is a boolean flag, so a bare token match is sufficient; it never
/// takes a value that could be `--json`. Scanning stops at the `--`
/// end-of-options marker, so a `--json` that appears only as a positional after
/// `--` does not flip a usage error into a JSON envelope.
fn wants_json<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    for arg in args {
        let arg = arg.as_ref();
        if arg == "--" {
            return false;
        }
        if arg == "--json" {
            return true;
        }
    }
    false
}

/// Parse argv to a [`Cli`], or render a clap error and return the exit code.
///
/// Returns `Ok(cli)` on a successful parse.
///
/// # Errors
///
/// Returns `Err(exit_code)` when the process should exit immediately, after
/// this function has already printed whatever the user should see:
///
/// - `Err(0)` for `--help`/`--version`: clap renders them to stdout (not
///   errors), then we exit cleanly.
/// - `Err(2)` for a clap **usage** error (missing/invalid argument, unknown
///   flag). This honors `--json`: when requested, it emits a `fez/v1` error
///   envelope on stdout (code `usage`) instead of clap's stderr text (issue
///   #52). Without `--json`, clap's human-facing rendering is preserved
///   unchanged.
pub fn parse_or_render() -> std::result::Result<Cli, i32> {
    let argv: Vec<std::ffi::OsString> = std::env::args_os().collect();
    match command().try_get_matches_from(&argv) {
        Ok(matches) => Ok(Cli::from_arg_matches(&matches).expect("clap validated args")),
        Err(err) => {
            use clap::error::ErrorKind;
            // Help/version are not failures: let clap print them, exit 0.
            if matches!(
                err.kind(),
                ErrorKind::DisplayHelp
                    | ErrorKind::DisplayVersion
                    | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            ) {
                let _ = err.print();
                return Err(0);
            }
            let json = wants_json(argv.iter().map(|s| s.to_string_lossy().into_owned()));
            if json {
                // Render a usage envelope on stdout. The host is localhost: a
                // parse error never reached a transport.
                let message = clap_error_message(&err);
                let env = crate::envelope::Envelope::error(
                    "Error",
                    "localhost",
                    crate::envelope::ApiError {
                        code: "usage".into(),
                        message,
                        detail: None,
                    },
                );
                println!("{}", env.to_json_string());
                Err(2)
            } else {
                let _ = err.print();
                Err(err.exit_code())
            }
        }
    }
}

/// Reduce a clap error to a single user-actionable line for the envelope.
///
/// clap renders the diagnostic, then a blank line, then a `Usage:` block and a
/// "for more information" footer. The actionable part is everything before that
/// first blank line; we join it into one line (so "missing arg" plus the listed
/// arg names stay together) and strip the leading `error: ` prefix.
fn clap_error_message(err: &clap::Error) -> String {
    let rendered = err.render().to_string();
    let mut parts: Vec<String> = Vec::new();
    for raw in rendered.lines() {
        let line = raw.trim();
        if line.is_empty() {
            // Blank line separates the diagnostic from the usage/footer block.
            break;
        }
        parts.push(line.to_string());
    }
    if parts.is_empty() {
        return "usage error".to_string();
    }
    let joined = parts.join(" ");
    joined
        .strip_prefix("error: ")
        .unwrap_or(&joined)
        .to_string()
}

/// The top-level subcommands fez accepts.
#[derive(Subcommand, Debug)]
pub enum TopCommand {
    /// List capability ids for on-demand discovery.
    Capabilities,
    /// Describe one capability (inputs, output kind, flags, examples).
    Describe {
        /// Dotted capability id to describe (e.g. `services.start`).
        capability: String,
    },
    /// Print the agent contract: discovery loop, envelope, exit codes, env vars.
    Guide,
    /// Emit the roff man page on stdout (used by packaging).
    #[command(hide = true)]
    Man,
    /// Manage systemd services.
    Services {
        /// The `services` action to perform.
        #[command(subcommand)]
        action: ServicesAction,
    },
    /// Manage RPM packages (via dnf5daemon).
    Packages {
        /// The `packages` action to perform.
        #[command(subcommand)]
        action: PackagesAction,
    },
    /// Inspect NetworkManager devices and connections.
    Network {
        /// The `network` action to perform.
        #[command(subcommand)]
        action: NetworkAction,
    },
    /// Manage the firewall (via firewalld).
    Firewall {
        /// The `firewall` action to perform.
        #[command(subcommand)]
        action: FirewallAction,
    },
    /// Show the system overview (host identity, OS, kernel, hardware, time).
    System {
        /// The `system` action to perform.
        #[command(subcommand)]
        action: SystemAction,
    },
    /// Inspect storage devices, partitions, and drive health (via UDisks2).
    Storage {
        /// The `storage` action to perform.
        #[command(subcommand)]
        action: StorageAction,
    },
    /// DNS resolver status and troubleshooting (via systemd-resolved).
    Dns {
        /// The `dns` action to perform.
        #[command(subcommand)]
        action: DnsAction,
    },
}

/// Actions under the `services` subcommand.
#[derive(Subcommand, Debug)]
pub enum ServicesAction {
    /// List units.
    List {
        /// Filter by systemd active state.
        #[arg(long, value_parser = clap::builder::PossibleValuesParser::new(crate::schema::SERVICE_STATES))]
        state: Option<String>,
    },
    /// Show one unit's status.
    Status {
        /// Unit to inspect.
        unit: String,
    },
    /// Read a unit's journal.
    Logs {
        /// Unit whose journal to read.
        unit: String,
        /// Only entries since this time (journalctl `--since` syntax).
        #[arg(long)]
        since: Option<String>,
        /// Minimum priority to include (journalctl `--priority` syntax).
        #[arg(long)]
        priority: Option<String>,
        /// Limit output to the last N entries.
        #[arg(long)]
        lines: Option<u32>,
        /// Stream new entries as they arrive.
        #[arg(long)]
        follow: bool,
    },
    /// Start a unit.
    Start {
        /// Unit to start.
        unit: String,
    },
    /// Stop a unit.
    Stop {
        /// Unit to stop.
        unit: String,
    },
    /// Restart a unit.
    Restart {
        /// Unit to restart.
        unit: String,
    },
    /// Reload a unit's configuration.
    Reload {
        /// Unit to reload.
        unit: String,
    },
    /// Enable a unit (optionally start it now).
    Enable {
        /// Unit to enable.
        unit: String,
        /// Also start the unit immediately.
        #[arg(long)]
        now: bool,
    },
    /// Disable a unit (optionally stop it now).
    Disable {
        /// Unit to disable.
        unit: String,
        /// Also stop the unit immediately.
        #[arg(long)]
        now: bool,
    },
}

/// Actions under the `packages` subcommand.
#[derive(Subcommand, Debug)]
pub enum PackagesAction {
    /// List packages.
    List {
        /// List only installed packages (the default).
        #[arg(long, conflicts_with = "available")]
        installed: bool,
        /// List available packages instead of installed.
        #[arg(long)]
        available: bool,
        /// Restrict to packages whose repo id exactly matches. Repeatable; a
        /// package is kept if its repo id equals any given value (OR).
        #[arg(long = "repo")]
        repo: Vec<String>,
        /// Restrict to packages whose name contains this substring.
        #[arg(long)]
        name: Option<String>,
        /// Maximum number of rows to return.
        #[arg(long)]
        limit: Option<usize>,
        /// Number of matching rows to skip before returning results.
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
    /// Show one package's full attributes.
    Info {
        /// Package spec to describe.
        spec: String,
    },
    /// Search packages by name, summary, or provides.
    Search {
        /// Pattern to match.
        pattern: String,
    },
    /// List available upgrades.
    CheckUpdate,
    /// List repositories and their enabled state.
    Repolist {
        /// Show only enabled repositories (the default).
        #[arg(long, conflicts_with_all = ["disabled", "all"])]
        enabled: bool,
        /// Show only disabled repositories.
        #[arg(long, conflicts_with = "all")]
        disabled: bool,
        /// Show all repositories.
        #[arg(long)]
        all: bool,
    },
    /// Install packages.
    Install {
        /// Package specs to install.
        #[arg(required = true)]
        specs: Vec<String>,
    },
    /// Remove packages.
    Remove {
        /// Package specs to remove.
        #[arg(required = true)]
        specs: Vec<String>,
    },
    /// Upgrade packages (all if none given).
    Upgrade {
        /// Package specs to upgrade; empty means upgrade everything.
        specs: Vec<String>,
    },
}

/// Actions under the `network` subcommand.
#[derive(Subcommand, Debug)]
pub enum NetworkAction {
    /// List network devices.
    List {
        /// Include every device, including unmanaged virtual interfaces.
        #[arg(long)]
        all: bool,
    },
    /// Show one device's full network detail.
    Show {
        /// Device interface name to inspect (e.g. `enp1s0`).
        device: String,
    },
}

/// Actions under the `system` subcommand.
#[derive(Subcommand, Debug)]
pub enum SystemAction {
    /// Show host identity, OS, kernel, hardware, and time/NTP status.
    Show,
    /// Show a live performance snapshot (CPU, memory, load, disk, network) via PCP.
    Metrics,
    /// List active login sessions.
    Sessions,
    /// List logged-in users.
    Users,
    /// List shutdown/sleep inhibitors.
    Inhibitors,
    /// List boot loader entries.
    #[command(name = "boot-entries")]
    BootEntries,
}

/// Actions under the `storage` subcommand.
#[derive(Subcommand, Debug)]
pub enum StorageAction {
    /// List block devices with filesystem type, label, UUID, size, and mount point.
    List,
    /// Show one block device's full detail (partition, drive, encryption).
    Show {
        /// Block device path or short name (e.g. `/dev/sda1` or `sda1`).
        device: String,
    },
    /// Show NVMe/SMART drive health (temperature, power-on hours, critical warnings, self-test status).
    Health {
        /// Filter by drive model, serial, or path substring.
        #[arg(long)]
        drive: Option<String>,
    },
}

/// Actions under the `dns` subcommand.
#[derive(Subcommand, Debug)]
pub enum DnsAction {
    /// Show DNS resolver configuration and cache statistics.
    Status {
        /// Include links with no DNS servers configured.
        #[arg(long)]
        all: bool,
    },
    /// Flush the DNS resolver cache.
    Flush,
    /// Resolve a hostname to addresses.
    Query {
        /// Hostname to resolve.
        hostname: String,
    },
}

/// Actions under the `firewall` subcommand.
#[derive(Subcommand, Debug)]
pub enum FirewallAction {
    /// Show firewall state, default zone, panic mode, and pending changes.
    Status,
    /// List zones with a per-zone summary.
    List,
    /// Show one zone's full detail.
    Show {
        /// Zone to inspect (e.g. `public`).
        zone: String,
    },
    /// List the service catalog firewalld knows about.
    Services,
    /// Add a service to a zone (runtime only; confirm to persist).
    AddService {
        /// Service name to add (e.g. `http`).
        service: String,
        /// Zone to add to (defaults to the default zone).
        #[arg(long)]
        zone: Option<String>,
        /// Auto-revert the runtime rule after this many seconds.
        #[arg(long)]
        timeout: Option<u32>,
    },
    /// Remove a service from a zone (runtime only; confirm to persist).
    RemoveService {
        /// Service name to remove.
        service: String,
        /// Zone to remove from (defaults to the default zone).
        #[arg(long)]
        zone: Option<String>,
    },
    /// Add a port to a zone (runtime only; confirm to persist).
    AddPort {
        /// Port spec as `port/proto` (e.g. `8080/tcp`).
        port: String,
        /// Zone to add to (defaults to the default zone).
        #[arg(long)]
        zone: Option<String>,
        /// Auto-revert the runtime rule after this many seconds.
        #[arg(long)]
        timeout: Option<u32>,
    },
    /// Remove a port from a zone (runtime only; confirm to persist).
    RemovePort {
        /// Port spec as `port/proto` (e.g. `8080/tcp`).
        port: String,
        /// Zone to remove from (defaults to the default zone).
        #[arg(long)]
        zone: Option<String>,
    },
    /// Set the default zone (gated: requires --force).
    SetDefaultZone {
        /// Zone to make default.
        zone: String,
    },
    /// Reload permanent config into runtime (discards uncommitted runtime changes).
    Reload,
    /// Persist the current runtime config to permanent (runtimeToPermanent).
    Confirm,
    /// Toggle panic mode (drops all traffic when on).
    Panic {
        /// Panic state to set.
        #[arg(value_parser = ["on", "off"])]
        state: String,
    },
    /// Enable or disable masquerade (SNAT) for a zone (runtime only; confirm to persist).
    Masquerade {
        /// Masquerade state to set.
        #[arg(value_parser = ["on", "off"])]
        state: String,
        /// Zone to change (defaults to the default zone).
        #[arg(long)]
        zone: Option<String>,
        /// Auto-revert the runtime rule after this many seconds (ignored for `off`).
        #[arg(long)]
        timeout: Option<u32>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).expect("args parse")
    }

    #[test]
    fn wants_json_detects_flag_anywhere() {
        assert!(wants_json(["fez", "--json", "services", "status"]));
        assert!(wants_json(["fez", "services", "status", "--json"]));
        assert!(!wants_json(["fez", "services", "status"]));
    }

    #[test]
    fn wants_json_respects_double_dash() {
        // `--json` after the end-of-options marker is a positional, not the flag.
        assert!(!wants_json(["fez", "--", "--json"]));
        // `--json` before `--` still enables JSON mode.
        assert!(wants_json(["fez", "--json", "--", "x"]));
    }

    #[test]
    fn clap_error_message_joins_missing_args_and_strips_prefix() {
        // A missing required positional renders "error: ...not provided:" then
        // the arg names on the next line; the message must keep them together
        // and drop the `error: ` prefix.
        let err = Cli::try_parse_from(["fez", "services", "status"]).unwrap_err();
        let msg = clap_error_message(&err);
        assert!(!msg.starts_with("error:"), "prefix not stripped: {msg}");
        assert!(msg.contains("UNIT"), "arg name missing: {msg}");
        assert!(!msg.contains('\n'), "message should be one line: {msg}");
    }

    #[test]
    fn clap_error_message_renders_unknown_flag() {
        let err = Cli::try_parse_from(["fez", "services", "list", "--bogus"]).unwrap_err();
        let msg = clap_error_message(&err);
        assert!(msg.contains("--bogus"), "{msg}");
    }

    #[test]
    fn resolved_host_defaults_to_localhost() {
        assert_eq!(
            cli(&["fez", "services", "list"]).resolved_host(),
            "localhost"
        );
    }

    #[test]
    fn resolved_host_normalizes_local_alias() {
        // `--host local` must report the same label as the transport uses
        // (`localhost`), so the envelope/audit host never drifts from the
        // host the bridge actually runs on.
        assert_eq!(
            cli(&["fez", "--host", "local", "services", "list"]).resolved_host(),
            "localhost"
        );
    }

    #[test]
    fn resolved_host_passes_through_explicit_host() {
        assert_eq!(
            cli(&["fez", "--host", "fedora@box.example", "services", "list"]).resolved_host(),
            "fedora@box.example"
        );
    }

    #[test]
    fn firewall_masquerade_parses_state_zone_timeout() {
        let c = cli(&[
            "fez",
            "firewall",
            "masquerade",
            "on",
            "--zone",
            "public",
            "--timeout",
            "60",
        ]);
        match c.command {
            TopCommand::Firewall {
                action:
                    FirewallAction::Masquerade {
                        state,
                        zone,
                        timeout,
                    },
            } => {
                assert_eq!(state, "on");
                assert_eq!(zone.as_deref(), Some("public"));
                assert_eq!(timeout, Some(60));
            }
            other => panic!("unexpected parse: {other:?}"),
        }
    }

    #[test]
    fn firewall_masquerade_rejects_bad_state() {
        assert!(Cli::try_parse_from(["fez", "firewall", "masquerade", "maybe"]).is_err());
    }
}
