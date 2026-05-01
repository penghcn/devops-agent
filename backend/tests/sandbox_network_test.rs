use devops_agent::sandbox::network_whitelist::{NetworkCheckResult, NetworkWhitelist};

#[test]
fn curl_is_blocked() {
    let wl = NetworkWhitelist::new();
    assert_eq!(
        wl.check("curl", &["https://example.com".into()]),
        NetworkCheckResult::Blocked
    );
}

#[test]
fn wget_is_blocked() {
    let wl = NetworkWhitelist::new();
    assert_eq!(
        wl.check("wget", &["https://example.com".into()]),
        NetworkCheckResult::Blocked
    );
}

#[test]
fn ssh_is_blocked() {
    let wl = NetworkWhitelist::new();
    assert_eq!(
        wl.check("ssh", &["user@host".into()]),
        NetworkCheckResult::Blocked
    );
}

#[test]
fn nc_is_blocked() {
    let wl = NetworkWhitelist::new();
    assert_eq!(
        wl.check("nc", &["-zv".into(), "host".into(), "80".into()]),
        NetworkCheckResult::Blocked
    );
}

#[test]
fn scp_is_blocked() {
    let wl = NetworkWhitelist::new();
    assert_eq!(
        wl.check("scp", &["file.txt".into(), "host:/tmp/".into()]),
        NetworkCheckResult::Blocked
    );
}

#[test]
fn ls_is_allowed() {
    let wl = NetworkWhitelist::new();
    assert_eq!(wl.check("ls", &["-la".into()]), NetworkCheckResult::Allowed);
}

#[test]
fn allow_host_adds_to_whitelist() {
    let mut wl = NetworkWhitelist::new();
    wl.allow_host("example.com");
    assert_eq!(wl.allowed_hosts, vec!["example.com"]);
}

#[test]
fn allowed_host_exact_match() {
    let mut wl = NetworkWhitelist::new();
    wl.allow_host("api.internal");
    assert!(wl.allowed_hosts.contains(&"api.internal".into()));
    assert!(!wl.allowed_hosts.contains(&"evil.internal".into()));
}
