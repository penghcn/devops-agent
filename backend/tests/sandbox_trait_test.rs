use devops_agent::sandbox::factory::{SandboxBackend, SandboxFactory};
#[cfg(target_os = "linux")]
use devops_agent::sandbox::process_backend::ProcessBackend;
#[cfg(target_os = "linux")]
use devops_agent::sandbox::trait_sandbox::Sandbox;

#[cfg(target_os = "linux")]
#[tokio::test]
async fn process_backend_exec_echo() {
    let backend = ProcessBackend::new();
    let result = backend.exec("echo", &["hello".to_string()]).await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("hello"));
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn process_backend_stop() {
    let backend = ProcessBackend::new();
    assert!(backend.stop().await.is_ok());
}

#[test]
fn factory_creates_process_backend() {
    let factory = SandboxFactory::new().with_backend(SandboxBackend::Process);
    let _sandbox = factory.create().unwrap();
}

#[test]
fn factory_default_is_process_on_non_linux() {
    // On macOS the default falls back to Process
    let factory = SandboxFactory::new();
    let _sandbox = factory.create().unwrap();
}
