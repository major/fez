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

    /// Override the protected-unit policy and skip interactive confirmation.
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
        /// Restrict to packages from these repositories.
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
}
