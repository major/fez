use predicates::str::contains;

mod common;
use common::fez_fake;

#[test]
fn sessions_json_returns_session_list() {
    fez_fake()
        .args(["system", "sessions", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"SessionList\""))
        .stdout(contains("\"id\":\"42\""))
        .stdout(contains("\"user\":\"major\""))
        .stdout(contains("\"remote\":true"))
        .stdout(contains("\"remote_host\":\"192.168.1.100\""))
        .stdout(contains("\"service\":\"sshd\""))
        .stdout(contains("\"id\":\"7\""))
        .stdout(contains("\"user\":\"root\""))
        .stdout(contains("\"remote\":false"));
}

#[test]
fn sessions_human_renders_table() {
    let out = fez_fake()
        .args(["system", "sessions"])
        .output()
        .expect("run fez");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("ID"));
    assert!(stdout.contains("USER"));
    assert!(stdout.contains("major"));
    assert!(stdout.contains("192.168.1.100"));
    assert!(stdout.contains("root"));
}

#[test]
fn users_json_returns_user_list() {
    fez_fake()
        .args(["system", "users", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"UserList\""))
        .stdout(contains("\"uid\":1000"))
        .stdout(contains("\"user\":\"major\""))
        .stdout(contains("\"uid\":0"))
        .stdout(contains("\"user\":\"root\""));
}

#[test]
fn users_human_renders_table() {
    let out = fez_fake()
        .args(["system", "users"])
        .output()
        .expect("run fez");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("UID"));
    assert!(stdout.contains("major"));
    assert!(stdout.contains("root"));
}

#[test]
fn inhibitors_json_returns_inhibitor_list() {
    fez_fake()
        .args(["system", "inhibitors", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"InhibitorList\""))
        .stdout(contains("\"what\":\"sleep\""))
        .stdout(contains("\"who\":\"NetworkManager\""))
        .stdout(contains("\"mode\":\"delay\""));
}

#[test]
fn inhibitors_human_renders_table() {
    let out = fez_fake()
        .args(["system", "inhibitors"])
        .output()
        .expect("run fez");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("WHAT"));
    assert!(stdout.contains("sleep"));
    assert!(stdout.contains("NetworkManager"));
}

#[test]
fn boot_entries_json_returns_entry_list() {
    fez_fake()
        .args(["system", "boot-entries", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"BootEntryList\""))
        .stdout(contains("abc-6.12.1-200.fc41.x86_64.conf"))
        .stdout(contains("abc-6.11.8-100.fc41.x86_64.conf"));
}

#[test]
fn boot_entries_human_renders_list() {
    let out = fez_fake()
        .args(["system", "boot-entries"])
        .output()
        .expect("run fez");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("abc-6.12.1-200.fc41.x86_64.conf"));
}

#[test]
fn describe_sessions_returns_descriptor() {
    fez_fake()
        .args(["describe", "system.sessions", "--json"])
        .assert()
        .success()
        .stdout(contains("\"id\":\"system.sessions\""))
        .stdout(contains("\"output_kind\":\"SessionList\""));
}

#[test]
fn capabilities_lists_session_commands() {
    fez_fake()
        .args(["capabilities"])
        .assert()
        .success()
        .stdout(contains("system.sessions"))
        .stdout(contains("system.users"))
        .stdout(contains("system.inhibitors"))
        .stdout(contains("system.boot-entries"));
}
