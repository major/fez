use crate::cli::{Cli, TopCommand};
use crate::envelope::Envelope;
use crate::schema;
use serde_json::json;
use std::io::Write;

pub fn run(cli: Cli) -> i32 {
    let host = cli.resolved_host();
    match &cli.command {
        TopCommand::Capabilities => run_capabilities(&cli, &host),
        TopCommand::Describe { capability: id } => run_describe(&cli, &host, id),
        TopCommand::Guide => crate::guide::run(&host, cli.json),
        TopCommand::Man => {
            let cmd = crate::cli::command();
            let man = clap_mangen::Man::new(cmd);
            let mut buf = Vec::new();
            man.render(&mut buf).expect("render man page");
            std::io::stdout().write_all(&buf).expect("write man page");
            0
        }
        TopCommand::Services { action } => crate::capabilities::services::dispatch(&cli, action),
        TopCommand::Packages { action } => crate::capabilities::package::dispatch(&cli, action),
        TopCommand::Network { action } => crate::capabilities::network::dispatch(&cli, action),
        TopCommand::Firewall { action } => crate::capabilities::firewall::dispatch(&cli, action),
        TopCommand::System { action } => crate::capabilities::system::dispatch(&cli, action),
        TopCommand::Storage { action } => crate::capabilities::storage::dispatch(&cli, action),
        TopCommand::Dns { action } => crate::capabilities::dns::dispatch(&cli, action),
        TopCommand::Journal {
            unit,
            since,
            until,
            priority,
            lines,
            boot,
            grep,
            list_boots,
            list_fields,
            output_fields,
        } => {
            // Validate inputs.
            for u in unit {
                if let Err(e) = crate::capabilities::services::validate_unit(u) {
                    return crate::capabilities::render(&cli, Err(e));
                }
            }
            if let Some(ref s) = since {
                if let Err(e) = crate::capabilities::services::validate_log_since(s) {
                    return crate::capabilities::render(&cli, Err(e));
                }
            }
            if let Some(ref s) = until {
                if let Err(e) = crate::capabilities::services::validate_log_since(s) {
                    return crate::capabilities::render(&cli, Err(e));
                }
            }
            if let Some(ref p) = priority {
                if let Err(e) = crate::capabilities::services::validate_log_priority(p) {
                    return crate::capabilities::render(&cli, Err(e));
                }
            }
            // Parse --boot: None = not specified, Some("current") = current boot,
            // Some("-1") = specific boot offset.
            let boot_parsed = boot.as_deref().map(|b| {
                if b == "current" {
                    None
                } else {
                    b.parse::<i64>().ok()
                }
            });
            let args = crate::capabilities::journal::JournalArgs {
                units: unit,
                since: since.as_deref(),
                until: until.as_deref(),
                priority: priority.as_deref(),
                lines: *lines,
                boot: boot_parsed,
                grep: grep.as_deref(),
                list_boots: *list_boots,
                list_fields: *list_fields,
                output_fields,
            };
            match crate::capabilities::connect(&cli) {
                Ok(mut client) => {
                    let result = crate::capabilities::journal::run(
                        &mut client,
                        host.clone(),
                        cli.json,
                        &args,
                    );
                    crate::capabilities::render(&cli, result)
                }
                Err(e) => crate::capabilities::render(&cli, Err(e)),
            }
        }
    }
}

fn run_capabilities(cli: &Cli, host: &str) -> i32 {
    let ids: Vec<&str> = schema::registry().into_iter().map(|d| d.id).collect();
    if cli.json {
        println!(
            "{}",
            Envelope::ok("CapabilityList", host, json!({"capabilities": ids})).to_json_string()
        );
    } else {
        ids.iter().for_each(|id| println!("{id}"));
    }
    0
}

fn run_describe(cli: &Cli, host: &str, id: &str) -> i32 {
    match schema::find(id) {
        Some(d) => {
            if cli.json {
                let data = serde_json::to_value(&d).unwrap_or_else(
                    |e| json!({"error": format!("descriptor serialization error: {e}")}),
                );
                println!(
                    "{}",
                    Envelope::ok("CapabilityDescriptor", host, data).to_json_string()
                );
            } else {
                print!("{}", d.render_text());
            }
            0
        }
        None => {
            // Discovery error: honor --json with a fez/v1 envelope (#52).
            if cli.json {
                let env = Envelope::error(
                    "Error",
                    host,
                    crate::envelope::ApiError {
                        code: "not-found".into(),
                        message: format!("unknown capability: {id}"),
                        detail: Some(json!({ "capability": id })),
                    },
                );
                println!("{}", env.to_json_string());
            } else {
                eprintln!("unknown capability: {id}");
            }
            4
        }
    }
}
