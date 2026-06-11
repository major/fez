use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use clap_complete::Shell;

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
    crate::capability::help::inject(raw_command())
}

/// Parse argv through the enriched command. Exits via clap on `--help`/errors.
pub fn parse() -> Cli {
    let matches = command().get_matches();
    Cli::from_arg_matches(&matches).expect("clap validated args")
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
    /// Generate a shell completion script on stdout.
    Completions {
        /// Shell to generate completions for.
        #[arg(value_enum)]
        shell: Shell,
    },
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
    /// Run as an MCP server (JSON-RPC 2.0 over stdio): a frugal gateway exposing
    /// list_capabilities, describe_capability, and invoke meta-tools.
    Mcp,
}

/// Actions under the `services` subcommand.
#[derive(Subcommand, Debug)]
pub enum ServicesAction {
    /// List units.
    List {
        /// Filter by active state (e.g. `active`, `failed`).
        #[arg(long)]
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
