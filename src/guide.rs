//! The agent bootstrap contract printed by `fez guide`. Tells an LLM how to
//! discover and invoke capabilities, what the envelope looks like, what the
//! exit codes mean, and which env vars matter.
use crate::envelope::Envelope;
use crate::error::EXIT_CODES;
use serde_json::json;

/// Render the guide as plain text for humans and `--help`-style reading.
pub fn text() -> String {
    let mut s = String::new();
    s.push_str("fez agent guide\n\n");
    s.push_str("Start here:\n");
    s.push_str("  fez system show --json          orient: OS, kernel, hardware, time\n\n");
    s.push_str("Discovery loop:\n");
    s.push_str("  1. fez capabilities            list capability ids\n");
    s.push_str("  2. fez describe <id> --json    inputs, flags, output kind, examples\n");
    s.push_str("  3. fez <command> ... --json    invoke; parse the fez/v1 envelope\n\n");
    s.push_str("Envelope (fez/v1): every --json response is\n");
    s.push_str(
        "  {apiVersion:\"fez/v1\", kind, host, status:\"ok\"|\"error\", data?, error?, hints?}\n\n",
    );
    s.push_str("Global flags: --host <h> --json --dry-run --force --ssh-identities-only\n\n");
    s.push_str("Exit codes:\n");
    for e in EXIT_CODES {
        s.push_str(&format!("  {:>2}  {:<14} {}\n", e.code, e.label, e.meaning));
    }
    s.push_str("\nEnv vars: FEZ_ESCALATION, FEZ_SSH_CONFIG, FEZ_SSH_IDENTITIES_ONLY.\n");
    s
}

/// The guide as a structured JSON value for `--json`.
pub fn data() -> serde_json::Value {
    json!({
        "orient": "fez system show --json",
        "discovery": [
            "fez capabilities",
            "fez describe <id> --json",
            "fez <command> ... --json"
        ],
        "envelope": {
            "apiVersion": "fez/v1",
            "fields": ["kind", "host", "status", "data", "error", "hints"]
        },
        "globalFlags": ["--host", "--json", "--dry-run", "--force", "--ssh-identities-only"],
        "exitCodes": EXIT_CODES.iter().map(|e| json!({
            "code": e.code, "label": e.label, "meaning": e.meaning
        })).collect::<Vec<_>>(),
        "envVars": [
            "FEZ_ESCALATION",
            "FEZ_SSH_CONFIG",
            "FEZ_SSH_IDENTITIES_ONLY"
        ]
    })
}

/// Print the guide. Returns the process exit code (always 0).
pub fn run(host: &str, as_json: bool) -> i32 {
    if as_json {
        println!(
            "{}",
            Envelope::ok("AgentGuide", host, data()).to_json_string()
        );
    } else {
        print!("{}", text());
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_lists_every_exit_code() {
        let t = text();
        for e in EXIT_CODES {
            assert!(t.contains(e.label), "guide text missing {}", e.label);
        }
    }

    #[test]
    fn data_has_exit_codes_and_envelope() {
        let d = data();
        assert!(d["exitCodes"].as_array().unwrap().len() == EXIT_CODES.len());
        assert_eq!(d["envelope"]["apiVersion"], "fez/v1");
    }

    #[test]
    fn guide_documents_ssh_identities_only_controls() {
        let t = text();
        assert!(t.contains("--ssh-identities-only"));
        assert!(t.contains("FEZ_SSH_IDENTITIES_ONLY"));

        let d = data();
        assert!(d["globalFlags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|flag| flag == "--ssh-identities-only"));
        assert!(d["envVars"]
            .as_array()
            .unwrap()
            .iter()
            .any(|var| var == "FEZ_SSH_IDENTITIES_ONLY"));
    }
}
