//! Maps capability descriptors onto the derived clap command tree so that
//! `--help` renders the same long descriptions and examples that `describe`
//! emits. The registry is the single source of truth.
use crate::capability;
use clap::Command;

/// Map a clap subcommand path (e.g. `["services", "start"]`) to a dotted
/// capability id (e.g. `services.start`), or `None` if the path has no
/// descriptor (e.g. `capabilities`, `describe`, `services` itself).
pub fn path_to_id(path: &[&str]) -> Option<String> {
    let id = path.join(".");
    capability::find(&id).map(|_| id)
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
    inject_at(cmd, &mut Vec::new())
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
            let d = capability::find(&id).expect("id resolved");
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
            // still accepts the flag. Only the help/completion visibility
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
}
