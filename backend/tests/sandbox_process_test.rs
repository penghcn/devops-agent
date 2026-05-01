use devops_agent::sandbox::process_sandbox::{ProcessSandbox, ProcessSandboxConfig};

#[test]
fn normal_command_executes() {
    let sandbox = ProcessSandbox::new();
    let result = sandbox.execute("echo", &["hello".into()]).unwrap();
    assert!(!result.timed_out);
    assert!(result.stdout.contains("hello"));
}

#[test]
fn timeout_command_is_terminated() {
    let config = ProcessSandboxConfig {
        timeout_secs: 1,
        max_output_bytes: 1024,
        allowed_env_keys: vec!["PATH".into(), "HOME".into(), "LANG".into(), "LC_ALL".into()],
    };
    let sandbox = ProcessSandbox::with_config(config);
    let result = sandbox.execute("sleep", &["10".into()]).unwrap();
    assert!(result.timed_out, "sleep 10 should timeout after 1s");
    assert_eq!(result.exit_code, -1);
}

#[test]
fn env_is_purged() {
    let config = ProcessSandboxConfig {
        timeout_secs: 5,
        max_output_bytes: 4096,
        allowed_env_keys: vec!["PATH".into()],
    };
    let sandbox = ProcessSandbox::with_config(config);
    let result = sandbox.execute("env", &[]).unwrap();
    assert!(!result.timed_out);
    // HOME should NOT be present since it's not in allowed_env_keys
    assert!(!result.stdout.contains("HOME="));
    // PATH should be present
    assert!(result.stdout.contains("PATH="));
}

#[test]
fn large_output_is_truncated() {
    let config = ProcessSandboxConfig {
        timeout_secs: 5,
        max_output_bytes: 50,
        allowed_env_keys: vec!["PATH".into(), "HOME".into(), "LANG".into(), "LC_ALL".into()],
    };
    let sandbox = ProcessSandbox::with_config(config);
    // Generate more than 50 bytes of output
    let result = sandbox
        .execute(
            "echo",
            &["aaaabbbbccccddddeeeeFFFFgggghhhh111122223333444455556666".into()],
        )
        .unwrap();
    assert!(result.truncated, "output should be truncated");
    assert!(result.stdout.ends_with("[...truncated]"));
}
