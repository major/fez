//! Maps capability descriptors onto the derived clap command tree so that
//! `--help` renders the same long descriptions and examples that `describe`
//! emits. The registry is the single source of truth.
use crate::schema;
use clap::Command;

/// Map a clap subcommand path (e.g. `["services", "start"]`) to a dotted
/// capability id (e.g. `services.start`), or `None` if the path has no
/// descriptor (e.g. `capabilities`, `describe`, `services` itself).
pub fn path_to_id(path: &[&str]) -> Option<String> {
    let id = path.join(".");
    schema::find(&id).map(|_| id)
}

/// Render a descriptor's examples as an `after_help` block.
pub fn examples_block(examples: &[String]) -> String {
    let mut s = String::from("Examples:\n");
    for ex in examples {
        s.push_str("  ");
        s.push_str(ex);
        s.push('\n');
    }
    s
}

/// Walk `cmd`, attaching `long_about` and `after_help` from the registry to
/// every subcommand whose path resolves to a capability id.
pub fn inject(cmd: Command) -> Command {
    let cmd = inject_at(cmd, &mut Vec::new());
    group_root_commands(cmd)
}

/// Discovery subcommands shown in their own help section rather than the
/// default "Commands" heading.
const DISCOVERY_COMMANDS: [&str; 3] = ["capabilities", "describe", "guide"];

/// Reorganise root subcommands into two visual groups:
/// - **Subsystems** (services, packages, network, firewall) under the renamed
///   default heading.
/// - **Agent discovery** (capabilities, describe, guide) in an `after_help`
///   block, hidden from the default listing so they don't clutter the primary
///   surface.
fn group_root_commands(mut cmd: Command) -> Command {
    // Collect about strings before mutating, so the after_help block can
    // reproduce them.
    let discovery_entries: Vec<(String, String)> = cmd
        .get_subcommands()
        .filter(|c| DISCOVERY_COMMANDS.contains(&c.get_name()))
        .map(|c| {
            let name = c.get_name().to_string();
            let about = c.get_about().map(|s| s.to_string()).unwrap_or_default();
            (name, about)
        })
        .collect();

    // Hide discovery subcommands from the default heading.
    for name in DISCOVERY_COMMANDS {
        cmd = cmd.mut_subcommand(name, |c| c.hide(true));
    }

    // Rename the default heading and append the discovery section.
    let mut section = String::from("Agent Discovery:\n");
    for (name, about) in &discovery_entries {
        section.push_str(&format!("  {name:<14}{about}\n"));
    }
    cmd.subcommand_help_heading("Subsystems")
        .after_help(section)
}

/// The safety globals that only apply to mutating commands. They are declared
/// `global = true` on the root, so clap offers them on every subcommand; on a
/// read-only leaf they are noise (issue #61). We hide them from a leaf's help
/// when its descriptor does not advertise them, while leaving them parseable so
/// the global contract holds.
/// Each entry is `(arg_id, long_flag)`. The id matches the field name clap
/// derives for the global on the root `Cli` (`dry_run`, `force`).
const HIDEABLE_GLOBALS: [(&str, &str); 2] = [("dry_run", "dry-run"), ("force", "force")];

fn inject_at(mut cmd: Command, path: &mut Vec<String>) -> Command {
    // Apply descriptor text to the current node if its path resolves.
    if !path.is_empty() {
        let parts: Vec<&str> = path.iter().map(String::as_str).collect();
        if let Some(id) = path_to_id(&parts) {
            let d = schema::find(&id).expect("id resolved");
            cmd = cmd
                .long_about(d.long)
                .after_help(examples_block(&d.examples));
            // Hide any safety global the descriptor does not advertise, so a
            // leaf's help mirrors its descriptor `flags` exactly. The flags are
            // declared `global = true` on the root, so they are not present on
            // this child at tree-build time (clap propagates them lazily) and
            // cannot be reached via `mut_arg`. Re-declare a local, hidden,
            // non-global shadow with the same id: clap renders the local arg
            // (hidden) instead of inheriting the visible global, while parsing
            // still accepts the flag. Only the help visibility
            // changes; the value is read from the root global at parse time.
            for (id, long) in HIDEABLE_GLOBALS {
                let advertised = d.flags.iter().any(|f| f == &format!("--{long}"));
                if !advertised {
                    cmd = cmd.arg(
                        clap::Arg::new(id)
                            .long(long)
                            .action(clap::ArgAction::SetTrue)
                            .hide(true),
                    );
                }
            }
        }
    }
    // Recurse into children, rebuilding each via mut_subcommand.
    let child_names: Vec<String> = cmd
        .get_subcommands()
        .map(|c| c.get_name().to_string())
        .collect();
    for name in child_names {
        path.push(name.clone());
        cmd = cmd.mut_subcommand(&name, |child| inject_at(child, path));
        path.pop();
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_path<'a>(cmd: &'a Command, path: &[&str]) -> Option<&'a Command> {
        let mut current = cmd;
        for name in path {
            current = current.get_subcommands().find(|c| c.get_name() == *name)?;
        }
        Some(current)
    }

    fn help_for(args: &[&str]) -> String {
        let err = crate::cli::command()
            .try_get_matches_from(args)
            .expect_err("--help should render help");
        err.render().to_string()
    }

    #[test]
    fn path_resolves_known_capability() {
        assert_eq!(
            path_to_id(&["services", "start"]).as_deref(),
            Some("services.start")
        );
        assert_eq!(path_to_id(&["capabilities"]), None);
        assert_eq!(path_to_id(&["services"]), None);
        assert_eq!(path_to_id(&["services", "bogus"]), None);
    }

    #[test]
    fn examples_block_lists_each_example() {
        let block = examples_block(&["fez a".into(), "fez b".into()]);
        assert!(block.contains("fez a"));
        assert!(block.contains("fez b"));
        assert!(block.starts_with("Examples:"));
    }

    #[test]
    fn root_help_groups_subsystems_and_discovery() {
        let mut cmd = inject(crate::cli::raw_command());
        let help = cmd.render_help().to_string();
        assert!(help.contains("Subsystems:"), "missing Subsystems heading");
        assert!(
            help.contains("Agent Discovery:"),
            "missing Agent Discovery heading"
        );
        // Subsystem commands are visible in the main listing.
        assert!(help.contains("services"));
        assert!(help.contains("firewall"));
        // Discovery commands appear in the output but not in the Subsystems section.
        assert!(help.contains("capabilities"));
        assert!(help.contains("describe"));
        assert!(help.contains("guide"));
        let subsystems_section =
            &help[help.find("Subsystems:").unwrap()..help.find("Agent Discovery:").unwrap()];
        assert!(
            !subsystems_section.contains("capabilities"),
            "capabilities leaked into Subsystems"
        );
        assert!(
            !subsystems_section.contains("describe"),
            "describe leaked into Subsystems"
        );
        assert!(
            !subsystems_section.contains("guide"),
            "guide leaked into Subsystems"
        );
    }

    #[test]
    fn inject_attaches_long_about_to_services_start() {
        let cmd = inject(crate::cli::raw_command());
        let services = cmd
            .get_subcommands()
            .find(|c| c.get_name() == "services")
            .unwrap();
        let start = services
            .get_subcommands()
            .find(|c| c.get_name() == "start")
            .unwrap();
        assert!(start.get_long_about().is_some());
        assert!(start.get_after_help().is_some());
    }

    #[test]
    fn inject_attaches_descriptor_help_to_every_capability_leaf() {
        let cmd = inject(crate::cli::raw_command());
        for descriptor in schema::registry() {
            let path: Vec<&str> = descriptor.id.split('.').collect();
            let leaf = find_path(&cmd, &path)
                .unwrap_or_else(|| panic!("{} has no clap command", descriptor.id));

            let long_about = leaf
                .get_long_about()
                .unwrap_or_else(|| panic!("{} missing long_about", descriptor.id))
                .to_string();
            assert_eq!(
                long_about, descriptor.long,
                "{} long_about drifted",
                descriptor.id
            );

            let after_help = leaf
                .get_after_help()
                .unwrap_or_else(|| panic!("{} missing examples", descriptor.id))
                .to_string();
            assert!(
                after_help.starts_with("Examples:"),
                "{} examples block missing heading: {after_help}",
                descriptor.id
            );
            for example in &descriptor.examples {
                assert!(
                    after_help.contains(example),
                    "{} help missing example {example:?}",
                    descriptor.id
                );
            }
        }
    }

    #[test]
    fn read_only_leaf_help_hides_safety_globals_without_rejecting_them() {
        let help = help_for(&["fez", "services", "list", "--help"]);
        assert!(
            !help.contains("--dry-run"),
            "read-only help exposed --dry-run"
        );
        assert!(!help.contains("--force"), "read-only help exposed --force");

        let matches = crate::cli::command()
            .try_get_matches_from(["fez", "services", "list", "--dry-run", "--force"])
            .expect("hidden safety globals should remain parseable");
        let cli = <crate::cli::Cli as clap::FromArgMatches>::from_arg_matches(&matches)
            .expect("hidden safety globals should construct Cli");
        assert!(cli.dry_run);
        assert!(cli.force);
    }

    #[test]
    fn mutating_leaf_help_keeps_advertised_safety_globals_visible() {
        let help = help_for(&["fez", "services", "start", "--help"]);
        assert!(help.contains("--dry-run"), "mutation help hid --dry-run");
        assert!(help.contains("--force"), "mutation help hid --force");
    }
}
