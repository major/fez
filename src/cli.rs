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
