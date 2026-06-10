use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn prints_version() {
    Command::cargo_bin("fez")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(contains("fez"));
}

#[test]
fn help_lists_command_groups() {
    Command::cargo_bin("fez")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("services"))
        .stdout(contains("capabilities"))
        .stdout(contains("describe"));
}

#[test]
fn global_flags_present() {
    Command::cargo_bin("fez")
        .unwrap()
        .args(["services", "list", "--help"])
        .assert()
        .success()
        .stdout(contains("--host"))
        .stdout(contains("--json"));
}

#[test]
fn capabilities_lists_service_ids() {
    Command::cargo_bin("fez")
        .unwrap()
        .arg("capabilities")
        .assert()
        .success()
        .stdout(contains("services.list"))
        .stdout(contains("services.logs"));
}

#[test]
fn describe_emits_envelope_json() {
    Command::cargo_bin("fez")
        .unwrap()
        .args(["describe", "services.status", "--json"])
        .assert()
        .success()
        .stdout(contains("\"apiVersion\": \"fez/v1\""))
        .stdout(contains("ServiceStatus"));
}

#[test]
fn describe_unknown_exits_4() {
    Command::cargo_bin("fez")
        .unwrap()
        .args(["describe", "nope"])
        .assert()
        .code(4);
}

#[test]
fn capabilities_lists_mutation_ids() {
    Command::cargo_bin("fez")
        .unwrap()
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
    Command::cargo_bin("fez")
        .unwrap()
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
    Command::cargo_bin("fez")
        .unwrap()
        .args(["describe", "services.enable", "--json"])
        .assert()
        .success()
        .stdout(contains("\"output_kind\": \"ServiceEnablement\""))
        .stdout(contains("--now"));
}

#[test]
fn services_help_lists_mutation_verbs() {
    Command::cargo_bin("fez")
        .unwrap()
        .args(["services", "--help"])
        .assert()
        .success()
        .stdout(contains("start"))
        .stdout(contains("enable"));
}

#[test]
fn help_lists_mcp_subcommand() {
    Command::cargo_bin("fez")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("mcp"));
}

#[test]
fn capabilities_json_emits_envelope() {
    Command::cargo_bin("fez")
        .unwrap()
        .args(["capabilities", "--json"])
        .assert()
        .success()
        .stdout(contains("\"apiVersion\": \"fez/v1\""))
        .stdout(contains("CapabilityList"))
        .stdout(contains("services.list"));
}

#[test]
fn describe_human_output_includes_example() {
    Command::cargo_bin("fez")
        .unwrap()
        .args(["describe", "services.status"])
        .assert()
        .success()
        .stdout(contains("services.status"))
        .stdout(contains("examples:"))
        .stdout(contains("fez services status"));
}

#[test]
fn describe_unknown_json_still_exits_4() {
    Command::cargo_bin("fez")
        .unwrap()
        .args(["describe", "nope", "--json"])
        .assert()
        .code(4)
        .stderr(contains("unknown capability"));
}

#[test]
fn services_start_help_shows_examples_and_long() {
    Command::cargo_bin("fez")
        .unwrap()
        .args(["services", "start", "--help"])
        .assert()
        .success()
        .stdout(contains("Examples:"))
        .stdout(contains("--force"));
}

#[test]
fn guide_text_mentions_discovery_loop_and_exit_codes() {
    Command::cargo_bin("fez")
        .unwrap()
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
    Command::cargo_bin("fez")
        .unwrap()
        .args(["guide", "--json"])
        .assert()
        .success()
        .stdout(contains("\"apiVersion\": \"fez/v1\""))
        .stdout(contains("AgentGuide"))
        .stdout(contains("exitCodes"));
}

#[test]
fn describe_text_shows_long_and_all_examples() {
    Command::cargo_bin("fez")
        .unwrap()
        .args(["describe", "services.enable"])
        .assert()
        .success()
        .stdout(contains("--now"))
        .stdout(contains("boot"));
}
