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

// ============ CubeSandbox 配置验证测试 ============

#[test]
fn cubesandbox_config_complete() {
    use devops_agent::sandbox::CubeSandboxConfig;
    let config = CubeSandboxConfig {
        api_url: "http://localhost:3000".to_string(),
        template_id: "test-template".to_string(),
        ..CubeSandboxConfig::default()
    };
    assert!(config.is_complete());
}

#[test]
fn cubesandbox_config_incomplete() {
    use devops_agent::sandbox::CubeSandboxConfig;
    let config = CubeSandboxConfig::default();
    assert!(!config.is_complete());
}

// ============ Factory 降级测试 ============

#[tokio::test]
async fn factory_fallback_to_process() {
    use devops_agent::sandbox::CubeSandboxConfig;

    #[cfg(target_os = "linux")]
    use devops_agent::sandbox::MicrosandboxConfig;

    let factory = SandboxFactory::from_config(
        vec![SandboxBackend::CubeSandbox],
        #[cfg(target_os = "linux")]
        MicrosandboxConfig::default(),
        CubeSandboxConfig::default(), // 配置不完整
    );
    factory.init().await;
    let _sandbox = factory.create().unwrap(); // 应降级到 process
}

#[test]
fn factory_cubesandbox_backend_creates() {
    // Without init(), it falls back to Process (default)
    // After init() on macOS without CubeSandbox, it should select Process
    // We test that create() doesn't panic
    let _ = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let factory = SandboxFactory::new().with_backend(SandboxBackend::CubeSandbox);
            factory.init().await;
            factory.create().unwrap();
        });
}
