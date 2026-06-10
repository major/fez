use crate::capability;
use crate::cli::{Cli, TopCommand};
use crate::envelope::Envelope;
use serde_json::json;

pub fn run(cli: Cli) -> i32 {
    let host = cli.host.clone().unwrap_or_else(|| "localhost".into());
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
                    let data = serde_json::to_value(&d).unwrap();
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
        TopCommand::Services { .. } => crate::capabilities::services::dispatch(&cli),
        TopCommand::Mcp => crate::mcp::run(),
    }
}
