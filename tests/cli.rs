use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;

mod common;
use common::fez_plain as fez;

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
        .stdout(contains("\"apiVersion\":\"fez/v1\""))
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
        .stdout(contains("\"privileged\":true"))
        .stdout(contains("\"output_kind\":\"ServiceMutation\""))
        .stdout(contains("--dry-run"))
        .stdout(contains("--force"));
}

#[test]
fn describe_enable_lists_now_flag() {
    fez()
        .args(["describe", "services.enable", "--json"])
        .assert()
        .success()
        .stdout(contains("\"output_kind\":\"ServiceEnablement\""))
        .stdout(contains("--now"));
}

#[test]
fn describe_json_includes_typed_flag_schema() {
    fez()
        .args(["describe", "services.logs", "--json"])
        .assert()
        .success()
        .stdout(contains("\"flag_schema\""))
        .stdout(contains("\"name\":\"--lines\""))
        .stdout(contains("\"type\":\"integer\""))
        .stdout(contains("\"name\":\"--follow\""))
        .stdout(contains("\"type\":\"boolean\""));
}

#[test]
fn describe_json_marks_repeatable_and_conflicting_flags() {
    fez()
        .args(["describe", "packages.list", "--json"])
        .assert()
        .success()
        .stdout(contains("\"name\":\"--repo\""))
        .stdout(contains("\"repeatable\":true"))
        .stdout(contains("\"name\":\"--installed\""))
        .stdout(contains("\"conflicts_with\":[\"--available\"]"));

    fez()
        .args(["describe", "packages.repolist", "--json"])
        .assert()
        .success()
        .stdout(contains("\"name\":\"--enabled\""))
        .stdout(contains("\"conflicts_with\":[\"--disabled\",\"--all\"]"));
}

#[test]
fn describe_json_includes_input_choices() {
    fez()
        .args(["describe", "firewall.panic", "--json"])
        .assert()
        .success()
        .stdout(contains("\"name\":\"state\""))
        .stdout(contains("\"choices\":[\"on\",\"off\"]"));
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
fn mcp_help_lists_expanded_tools_flag() {
    fez()
        .args(["mcp", "--help"])
        .assert()
        .success()
        .stdout(contains("--expanded-tools"));
}

#[test]
fn capabilities_json_emits_envelope() {
    fez()
        .args(["capabilities", "--json"])
        .assert()
        .success()
        .stdout(contains("\"apiVersion\":\"fez/v1\""))
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

// Issue #62: top-level help promises describe prints inputs, output kind,
// flags, and privileged status, but plain-text describe only showed the
// summary/long/examples. Assert the plain-text output now carries the same
// essential metadata the --json form does, so an agent reading text output can
// act safely without switching to JSON.
#[test]
fn describe_text_includes_privileged_output_inputs_flags() {
    fez()
        .args(["describe", "services.start"])
        .assert()
        .success()
        .stdout(contains("privileged: true"))
        .stdout(contains("output: ServiceMutation"))
        .stdout(contains("inputs:"))
        .stdout(contains("unit: string required"))
        .stdout(contains("flags:"))
        .stdout(contains("--force"))
        .stdout(contains("--dry-run"));
}

// A read-only capability is not privileged and takes no required inputs; its
// plain-text describe should say so rather than omit the section.
#[test]
fn describe_text_marks_readonly_not_privileged() {
    fez()
        .args(["describe", "services.list"])
        .assert()
        .success()
        .stdout(contains("privileged: false"))
        .stdout(contains("output: ServiceList"));
}

// Issue #52: under --json the unknown-capability discovery error emits a fez/v1
// error envelope on stdout (still exit 4), instead of a bare stderr line. That
// path is covered by json_unknown_capability_emits_envelope below (which also
// asserts the capability id round-trips); the plain-text path keeps stderr (see
// plain_unknown_capability_keeps_stderr).

#[test]
fn services_start_help_shows_examples_and_long() {
    fez()
        .args(["services", "start", "--help"])
        .assert()
        .success()
        .stdout(contains("Examples:"))
        .stdout(contains("--force"));
}

// Issue #63: the global --force help was systemd-specific ("Override the
// protected-unit policy"), but --force also gates package and firewall
// guardrails. The top-level flag text must be generic; the per-command long
// help keeps the precise risk wording (protected units, dangerous
// transactions, firewall lockout).
#[test]
fn global_force_help_not_systemd_specific() {
    fez()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("--force"))
        .stdout(contains("Override command-specific safety guardrails"))
        .stdout(contains("protected-unit policy").not());
}

// Service command help must still describe the protected-unit behavior so the
// detailed risk wording is not lost when the global text goes generic.
#[test]
fn services_start_help_keeps_protected_unit_wording() {
    fez()
        .args(["services", "start", "--help"])
        .assert()
        .success()
        .stdout(contains("Protected units"));
}

// Issue #61: --dry-run/--force are clap globals, so they leaked into the help
// of read-only commands where they have no effect, adding action-space noise an
// LLM can trip over (e.g. `fez network list --force`). The inject() pass now
// hides any global flag not advertised by a leaf's descriptor, so read-only
// command help no longer lists --force/--dry-run.
#[test]
fn readonly_command_help_hides_force_and_dry_run() {
    for path in [
        ["network", "list"],
        ["packages", "info"],
        ["firewall", "status"],
        ["services", "status"],
    ] {
        fez()
            .args([path[0], path[1], "--help"])
            .assert()
            .success()
            .stdout(contains("--force").not())
            .stdout(contains("--dry-run").not());
    }
}

// Mutating commands must still advertise the flags they actually honor.
#[test]
fn mutating_command_help_keeps_force_and_dry_run() {
    fez()
        .args(["services", "start", "--help"])
        .assert()
        .success()
        .stdout(contains("--force"))
        .stdout(contains("--dry-run"));
    // packages install honors both too.
    fez()
        .args(["packages", "install", "--help"])
        .assert()
        .success()
        .stdout(contains("--force"))
        .stdout(contains("--dry-run"));
}

// Hiding from help must not change parse behavior: the global contract holds, so
// passing a hidden flag to a read-only command is still accepted (no-op), not a
// usage error. Use --json so the command short-circuits before touching a bridge.
#[test]
fn hidden_global_flag_still_parses_on_readonly() {
    // network list is unprivileged and needs no bridge; --force is a no-op here.
    // It should not fail with a clap usage error (exit 2). It may fail later for
    // transport reasons, but must not reject the flag itself.
    let out = fez()
        .args(["network", "list", "--force", "--json"])
        .output()
        .expect("run");
    assert_ne!(
        out.status.code(),
        Some(2),
        "read-only command rejected a hidden global flag as a usage error: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// Drift guard: the flags shown in each capability leaf's help must match the
// flags advertised in that descriptor (the registry is canonical). A global
// flag missing from the descriptor must be hidden; a flag present must be
// visible.
#[test]
fn help_flags_match_descriptor_flags() {
    let cmd = fez::cli::command();
    for d in fez::capability::registry() {
        let parts: Vec<&str> = d.id.split('.').collect();
        // Walk to the leaf subcommand.
        let mut node = &cmd;
        for p in &parts {
            node = node
                .get_subcommands()
                .find(|c| c.get_name() == *p)
                .unwrap_or_else(|| panic!("no subcommand for {}", d.id));
        }
        for arg in node.get_arguments() {
            let long = match arg.get_long() {
                Some(l) => format!("--{l}"),
                None => continue,
            };
            // Only the safety globals are conditionally hidden.
            if long != "--force" && long != "--dry-run" {
                continue;
            }
            let advertised = d.flags.iter().any(|f| f == &long);
            let hidden = arg.is_hide_set();
            assert_eq!(
                hidden, !advertised,
                "{}: {long} hidden={hidden} but descriptor advertised={advertised}",
                d.id
            );
        }
    }
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
        .stdout(contains("\"apiVersion\":\"fez/v1\""))
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

// Regression for the BrokenPipe panic (issue #20): when a downstream reader
// closes the pipe early, fez must not panic (exit 101) or print a panic
// backtrace. With SIGPIPE reset to SIG_DFL it dies quietly via the signal, the
// conventional Unix behavior. We spawn the real binary, read a single byte from
// its stdout, then drop the read end so the rest of the write hits EPIPE.
#[cfg(unix)]
#[test]
fn broken_pipe_does_not_panic() {
    use std::io::Read;
    use std::process::{Command as StdCommand, Stdio};

    let exe = assert_cmd::cargo::cargo_bin("fez");
    let mut child = StdCommand::new(exe)
        .args(["completions", "zsh"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn fez");

    // Read one byte, then drop stdout to close the read end of the pipe.
    {
        let mut stdout = child.stdout.take().expect("child stdout");
        let mut one = [0u8; 1];
        let _ = stdout.read(&mut one);
    }

    let output = child.wait_with_output().expect("wait for fez");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !stderr.contains("panicked"),
        "fez panicked on broken pipe: {stderr}"
    );
    assert_ne!(
        output.status.code(),
        Some(101),
        "fez exited 101 (Rust panic) on broken pipe"
    );
}

// Split an example command line into argv, honoring single-quoted segments
// (the only quoting any descriptor example uses, e.g. --since '1 hour ago').
fn tokenize_example(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    let mut has_token = false;
    for c in s.chars() {
        match c {
            '\'' => {
                in_quote = !in_quote;
                has_token = true;
            }
            ch if ch.is_whitespace() && !in_quote => {
                if has_token {
                    out.push(std::mem::take(&mut cur));
                    has_token = false;
                }
            }
            ch => {
                cur.push(ch);
                has_token = true;
            }
        }
    }
    if has_token {
        out.push(cur);
    }
    out
}

// Regression for issue #53: every example in every capability descriptor must
// parse cleanly against the real clap tree. The mutation descriptors used to
// emit "fez services start --json" with no <UNIT>, so an agent copying the
// advertised example hit "required arguments were not provided". Parse each
// example for real and fail if clap rejects it (missing required arg, unknown
// flag, etc.), so a future descriptor edit that drops a required positional is
// caught by `make test`, not only on a live host.
#[test]
fn every_descriptor_example_parses() {
    let cmd = fez::cli::command();
    for d in fez::capability::registry() {
        for example in &d.examples {
            let argv = tokenize_example(example);
            assert_eq!(
                argv.first().map(String::as_str),
                Some("fez"),
                "example for {} does not start with `fez`: {example}",
                d.id
            );
            // try_get_matches_from consumes a clone; argv[0] is the bin name.
            let res = cmd.clone().try_get_matches_from(&argv);
            assert!(
                res.is_ok(),
                "example for {} fails to parse: `{example}`\n  {}",
                d.id,
                res.unwrap_err()
            );
        }
    }
}

// ---- issue #52: --json error envelopes for usage and discovery errors ----

// A clap parse error (missing required positional) with --json must emit a
// fez/v1 error envelope on stdout, exit non-zero, and carry a stable code.
#[test]
fn json_missing_required_arg_emits_envelope() {
    fez()
        .args(["--json", "services", "status"])
        .assert()
        .code(2)
        .stdout(contains("\"apiVersion\":\"fez/v1\""))
        .stdout(contains("\"kind\":\"Error\""))
        .stdout(contains("\"status\":\"error\""))
        .stdout(contains("\"code\":\"usage\""))
        // stderr must NOT carry the clap text when JSON was requested.
        .stderr(contains("required arguments").not());
}

// --json may appear after the subcommand; the envelope must still be emitted.
#[test]
fn json_after_subcommand_still_emits_envelope() {
    fez()
        .args(["services", "status", "--json"])
        .assert()
        .code(2)
        .stdout(contains("\"apiVersion\":\"fez/v1\""))
        .stdout(contains("\"kind\":\"Error\""))
        .stdout(contains("\"status\":\"error\""))
        .stdout(contains("\"code\":\"usage\""));
}

// An unknown flag is also a usage error under --json.
#[test]
fn json_unknown_flag_emits_envelope() {
    fez()
        .args(["services", "list", "--json", "--bogus"])
        .assert()
        .code(2)
        .stdout(contains("\"kind\":\"Error\""))
        .stdout(contains("\"code\":\"usage\""));
}

// describe of an unknown capability with --json must emit an envelope, not a
// bare stderr line.
#[test]
fn json_unknown_capability_emits_envelope() {
    fez()
        .args(["describe", "nope.nope", "--json"])
        .assert()
        .code(4)
        .stdout(contains("\"kind\":\"Error\""))
        .stdout(contains("\"code\":\"not-found\""))
        .stdout(contains("nope.nope"));
}

// Without --json, usage errors keep clap's human text on stderr (exit 2), so
// interactive use is unchanged.
#[test]
fn plain_missing_arg_keeps_clap_stderr() {
    fez()
        .args(["services", "status"])
        .assert()
        .code(2)
        .stderr(contains("required arguments"));
}

// Without --json, an unknown capability keeps the plain stderr line.
#[test]
fn plain_unknown_capability_keeps_stderr() {
    fez()
        .args(["describe", "nope.nope"])
        .assert()
        .code(4)
        .stderr(contains("unknown capability"));
}

// --help and --version must still succeed (exit 0) even with --json present;
// they are not errors and must not be converted to error envelopes.
#[test]
fn json_help_and_version_still_succeed() {
    fez().args(["--json", "--help"]).assert().success();
    fez().args(["--json", "--version"]).assert().success();
}
