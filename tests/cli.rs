use assert_cmd::Command;
use predicates::str::contains;

fn fez() -> Command {
    Command::cargo_bin("fez").unwrap()
}

#[test]
fn prints_version() {
    fez()
        .arg("--version")
        .assert()
        .success()
        .stdout(contains("fez"));
}

#[test]
fn help_lists_command_groups() {
    fez()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("services"))
        .stdout(contains("capabilities"))
        .stdout(contains("describe"));
}

#[test]
fn global_flags_present() {
    fez()
        .args(["services", "list", "--help"])
        .assert()
        .success()
        .stdout(contains("--host"))
        .stdout(contains("--json"));
}

#[test]
fn capabilities_lists_service_ids() {
    fez()
        .arg("capabilities")
        .assert()
        .success()
        .stdout(contains("services.list"))
        .stdout(contains("services.logs"));
}

#[test]
fn describe_emits_envelope_json() {
    fez()
        .args(["describe", "services.status", "--json"])
        .assert()
        .success()
        .stdout(contains("\"apiVersion\": \"fez/v1\""))
        .stdout(contains("ServiceStatus"));
}

#[test]
fn describe_unknown_exits_4() {
    fez().args(["describe", "nope"]).assert().code(4);
}

#[test]
fn capabilities_lists_mutation_ids() {
    fez()
        .arg("capabilities")
        .assert()
        .success()
        .stdout(contains("services.start"))
        .stdout(contains("services.stop"))
        .stdout(contains("services.restart"))
        .stdout(contains("services.reload"))
        .stdout(contains("services.enable"))
        .stdout(contains("services.disable"));
}

#[test]
fn describe_start_is_privileged() {
    fez()
        .args(["describe", "services.start", "--json"])
        .assert()
        .success()
        .stdout(contains("\"privileged\": true"))
        .stdout(contains("\"output_kind\": \"ServiceMutation\""))
        .stdout(contains("--dry-run"))
        .stdout(contains("--force"));
}

#[test]
fn describe_enable_lists_now_flag() {
    fez()
        .args(["describe", "services.enable", "--json"])
        .assert()
        .success()
        .stdout(contains("\"output_kind\": \"ServiceEnablement\""))
        .stdout(contains("--now"));
}

#[test]
fn services_help_lists_mutation_verbs() {
    fez()
        .args(["services", "--help"])
        .assert()
        .success()
        .stdout(contains("start"))
        .stdout(contains("enable"));
}

#[test]
fn help_lists_mcp_subcommand() {
    fez()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("mcp"));
}

#[test]
fn capabilities_json_emits_envelope() {
    fez()
        .args(["capabilities", "--json"])
        .assert()
        .success()
        .stdout(contains("\"apiVersion\": \"fez/v1\""))
        .stdout(contains("CapabilityList"))
        .stdout(contains("services.list"));
}

#[test]
fn describe_human_output_includes_example() {
    fez()
        .args(["describe", "services.status"])
        .assert()
        .success()
        .stdout(contains("services.status"))
        .stdout(contains("examples:"))
        .stdout(contains("fez services status"));
}

#[test]
fn describe_unknown_json_still_exits_4() {
    fez()
        .args(["describe", "nope", "--json"])
        .assert()
        .code(4)
        .stderr(contains("unknown capability"));
}

#[test]
fn services_start_help_shows_examples_and_long() {
    fez()
        .args(["services", "start", "--help"])
        .assert()
        .success()
        .stdout(contains("Examples:"))
        .stdout(contains("--force"));
}

#[test]
fn guide_text_mentions_discovery_loop_and_exit_codes() {
    fez()
        .arg("guide")
        .assert()
        .success()
        .stdout(contains("capabilities"))
        .stdout(contains("describe"))
        .stdout(contains("protected-unit"))
        .stdout(contains("fez/v1"));
}

#[test]
fn guide_json_emits_agent_guide_envelope() {
    fez()
        .args(["guide", "--json"])
        .assert()
        .success()
        .stdout(contains("\"apiVersion\": \"fez/v1\""))
        .stdout(contains("AgentGuide"))
        .stdout(contains("exitCodes"));
}

#[test]
fn describe_text_shows_long_and_all_examples() {
    fez()
        .args(["describe", "services.enable"])
        .assert()
        .success()
        .stdout(contains("--now"))
        .stdout(contains("boot"));
}

#[test]
fn completions_bash_emits_script() {
    fez()
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(contains("_fez"));
}

#[test]
fn man_emits_roff() {
    fez()
        .arg("man")
        .assert()
        .success()
        .stdout(contains(".TH"))
        .stdout(contains("fez"));
}

#[test]
fn every_capability_id_has_a_clap_path() {
    // Each dotted id maps to a real subcommand path under the enriched command.
    let cmd = fez::cli::command();
    for d in fez::capability::registry() {
        let parts: Vec<&str> = d.id.split('.').collect();
        let mut node = &cmd;
        let mut found = true;
        for p in &parts {
            match node.get_subcommands().find(|c| c.get_name() == *p) {
                Some(c) => node = c,
                None => {
                    found = false;
                    break;
                }
            }
        }
        assert!(found, "capability {} has no clap path", d.id);
    }
}

#[test]
fn describe_example_matches_help_after_help() {
    // describe and --help both read the registry, so the first example must
    // appear in the rendered after_help for that command.
    let cmd = fez::cli::command();
    let d = fez::capability::find("services.start").unwrap();
    let services = cmd
        .get_subcommands()
        .find(|c| c.get_name() == "services")
        .unwrap();
    let start = services
        .get_subcommands()
        .find(|c| c.get_name() == "start")
        .unwrap();
    let after = start.get_after_help().unwrap().to_string();
    assert!(after.contains(&d.examples[0]));
}
