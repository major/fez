use crate::capability;
use crate::cli::{Cli, TopCommand};
use crate::envelope::Envelope;
use serde_json::json;
use std::io::Write;

pub fn run(cli: Cli) -> i32 {
    let host = cli.resolved_host();
    match &cli.command {
        TopCommand::Capabilities => {
            let ids: Vec<String> = capability::registry().into_iter().map(|d| d.id).collect();
            if cli.json {
                println!(
                    "{}",
                    Envelope::ok("CapabilityList", &host, json!({"capabilities": ids}))
                        .to_json_string()
                );
            } else {
                ids.iter().for_each(|id| println!("{id}"));
            }
            0
        }
        TopCommand::Describe { capability: id } => match capability::find(id) {
            Some(d) => {
                if cli.json {
                    let data = serde_json::to_value(&d).unwrap_or_else(
                        |e| json!({"error": format!("descriptor serialization error: {e}")}),
                    );
                    println!(
                        "{}",
                        Envelope::ok("CapabilityDescriptor", &host, data).to_json_string()
                    );
                } else {
                    println!("{}: {}", d.id, d.summary);
                    println!("{}", d.long);
                    println!("examples:");
                    for ex in &d.examples {
                        println!("  {ex}");
                    }
                }
                0
            }
            None => {
                eprintln!("unknown capability: {id}");
                4
            }
        },
        TopCommand::Guide => crate::guide::run(&host, cli.json),
        TopCommand::Completions { shell } => {
            let mut cmd = crate::cli::command();
            let name = cmd.get_name().to_string();
            clap_complete::generate(*shell, &mut cmd, name, &mut std::io::stdout());
            0
        }
        TopCommand::Man => {
            let cmd = crate::cli::command();
            let man = clap_mangen::Man::new(cmd);
            let mut buf = Vec::new();
            man.render(&mut buf).expect("render man page");
            std::io::stdout().write_all(&buf).expect("write man page");
            0
        }
        TopCommand::Services { action } => crate::capabilities::services::dispatch(&cli, action),
        TopCommand::Packages { action } => crate::capabilities::packages::dispatch(&cli, action),
        TopCommand::Mcp => crate::mcp::run(),
    }
}
