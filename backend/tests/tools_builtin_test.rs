use devops_agent::sandbox::{
    FileSystemIsolator, FsIsolationConfig, NetworkWhitelist, PathValidator, ProcessSandbox,
};
use devops_agent::security::audit::AuditLog;
use devops_agent::security::policy::PolicyEngine;
use devops_agent::security::roles::Role;
use devops_agent::tools::builtin::{
    BashTool, GitTool, ReadTool, Tool, ToolInput, ToolOutput, WriteTool,
};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

/// 测试夹具：创建临时 workspace 并返回所有依赖组件
/// 注意：TempDir 必须持有，否则目录会被清理
struct TestFixture {
    pub workspace: PathBuf,
    pub output_dir: PathBuf,
    pub validator: PathValidator,
    pub isolator: FileSystemIsolator,
    pub sandbox: ProcessSandbox,
    pub policy_engine: PolicyEngine,
    pub audit_log: AuditLog,
    pub network_whitelist: NetworkWhitelist,
    #[allow(dead_code)]
    pub tmp: TempDir, // 保持引用，防止目录被清理
}

impl TestFixture {
    fn setup() -> Self {
        let tmp = TempDir::new().expect("创建临时目录失败");
        let workspace = tmp.path().join("workspace");
        let output_dir = workspace.join("output");
        let tmp_dir = workspace.join("tmp");

        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&output_dir).unwrap();
        fs::create_dir_all(&tmp_dir).unwrap();

        let ws_str = workspace.to_str().unwrap();

        let validator = PathValidator::new(ws_str);

        let isolator = FileSystemIsolator::new(FsIsolationConfig {
            workspace_root: workspace.clone(),
            tmp_dir: tmp_dir.clone(),
            output_dir: output_dir.clone(),
            read_only_mounts: vec![],
        });

        let sandbox = ProcessSandbox::new();
        let audit_log = AuditLog::new();
        let policy_engine = PolicyEngine::new(Arc::new(audit_log.clone()));
        let network_whitelist = NetworkWhitelist::new();

        Self {
            workspace,
            output_dir,
            validator,
            isolator,
            sandbox,
            policy_engine,
            audit_log,
            network_whitelist,
            tmp,
        }
    }
}

mod read_tool_tests {
    use super::*;

    #[tokio::test]
    async fn test_read_existing_file() {
        let fixture = TestFixture::setup();

        let test_file = fixture.workspace.join("test.txt");
        fs::write(&test_file, "hello world").unwrap();

        let read_tool = ReadTool::new(fixture.validator.clone(), fixture.isolator.clone(), fixture.policy_engine.clone());
        let input = ToolInput {
            path: Some(test_file.to_str().unwrap().to_string()),
            content: None,
            arguments: vec![],
            user_role: Role::Admin,
        };

        let output = read_tool.execute(&input).await;
        assert!(output.success, "读取文件应该成功: {:?}", output.error);
        assert_eq!(output.result, "hello world");
    }

    #[tokio::test]
    async fn test_read_nonexistent_file() {
        let fixture = TestFixture::setup();

        let read_tool = ReadTool::new(fixture.validator.clone(), fixture.isolator.clone(), fixture.policy_engine.clone());
        let input = ToolInput {
            path: Some(
                fixture
                    .workspace
                    .join("nonexistent.txt")
                    .to_str()
                    .unwrap()
                    .to_string(),
            ),
            content: None,
            arguments: vec![],
            user_role: Role::Admin,
        };

        let output = read_tool.execute(&input).await;
        assert!(!output.success, "读取不存在的文件应该失败");
        assert!(output.error.as_ref().unwrap().contains("无法访问"));
    }

    #[tokio::test]
    async fn test_read_path_traversal_rejected() {
        let fixture = TestFixture::setup();

        let read_tool = ReadTool::new(fixture.validator.clone(), fixture.isolator.clone(), fixture.policy_engine.clone());
        let input = ToolInput {
            path: Some(format!("{}/../etc/passwd", fixture.workspace.display())),
            content: None,
            arguments: vec![],
            user_role: Role::Admin,
        };

        let output = read_tool.execute(&input).await;
        assert!(!output.success, "路径穿越应该被拒绝");
        assert!(
            output.error.as_ref().unwrap().contains("路径校验失败"),
            "错误信息应包含路径校验失败"
        );
    }

    #[tokio::test]
    async fn test_read_large_file_rejected() {
        let fixture = TestFixture::setup();

        let test_file = fixture.workspace.join("large.txt");
        // 11MB 文件，超过默认 10MB 限制
        let large_content = "x".repeat(11 * 1024 * 1024);
        fs::write(&test_file, &large_content).unwrap();

        let read_tool = ReadTool::new(fixture.validator.clone(), fixture.isolator.clone(), fixture.policy_engine.clone());
        let input = ToolInput {
            path: Some(test_file.to_str().unwrap().to_string()),
            content: None,
            arguments: vec![],
            user_role: Role::Admin,
        };

        let output = read_tool.execute(&input).await;
        assert!(!output.success, "超大文件应该被拒绝");
        assert!(
            output.error.as_ref().unwrap().contains("文件过大"),
            "错误信息应包含文件过大"
        );
    }
}

mod write_tool_tests {
    use super::*;

    #[tokio::test]
    async fn test_write_to_output_dir() {
        let fixture = TestFixture::setup();

        let write_tool = WriteTool::new(fixture.validator.clone(), fixture.isolator.clone(), fixture.policy_engine.clone());
        let target = fixture.output_dir.join("test.txt");

        let input = ToolInput {
            path: Some(target.to_str().unwrap().to_string()),
            content: Some("test content".into()),
            arguments: vec![],
            user_role: Role::Admin,
        };

        let output = write_tool.execute(&input).await;
        assert!(
            output.success,
            "写入 output 目录应该成功: {:?}",
            output.error
        );
        assert_eq!(output.result, "写入成功");

        // 验证文件内容
        let content = fs::read_to_string(&target).unwrap();
        assert_eq!(content, "test content");
    }

    #[tokio::test]
    async fn test_write_oversized_content_rejected() {
        let fixture = TestFixture::setup();

        let write_tool = WriteTool::new(fixture.validator.clone(), fixture.isolator.clone(), fixture.policy_engine.clone());
        let target = fixture.output_dir.join("large.txt");

        // 6MB 内容，超过默认 5MB 限制
        let large_content = "x".repeat(6 * 1024 * 1024);

        let input = ToolInput {
            path: Some(target.to_str().unwrap().to_string()),
            content: Some(large_content),
            arguments: vec![],
            user_role: Role::Admin,
        };

        let output = write_tool.execute(&input).await;
        assert!(!output.success, "超大内容应该被拒绝");
        assert!(
            output.error.as_ref().unwrap().contains("内容过大"),
            "错误信息应包含内容过大"
        );
    }

    #[tokio::test]
    async fn test_write_outside_output_dir_rejected() {
        let fixture = TestFixture::setup();

        let write_tool = WriteTool::new(fixture.validator.clone(), fixture.isolator.clone(), fixture.policy_engine.clone());
        let target = fixture.workspace.join("not_output.txt");

        let input = ToolInput {
            path: Some(target.to_str().unwrap().to_string()),
            content: Some("test".into()),
            arguments: vec![],
            user_role: Role::Admin,
        };

        let output = write_tool.execute(&input).await;
        assert!(!output.success, "写入非 output 目录应该被拒绝");
        assert!(
            output.error.as_ref().unwrap().contains("output"),
            "错误信息应提及 output 目录"
        );
    }
}

mod bash_tool_tests {
    use super::*;

    #[tokio::test]
    async fn test_bash_admin_execute_success() {
        let fixture = TestFixture::setup();

        let bash_tool = BashTool::new(
            fixture.sandbox.clone(),
            fixture.network_whitelist.clone(),
            fixture.policy_engine.clone(),
        );
        let input = ToolInput {
            path: None,
            content: None,
            arguments: vec!["echo".into(), "hello".into()],
            user_role: Role::Admin,
        };

        let output = bash_tool.execute(&input).await;
        assert!(output.success, "Admin 执行命令应该成功: {:?}", output.error);
        assert!(output.result.contains("hello"));
    }

    #[tokio::test]
    async fn test_bash_viewer_denied() {
        let fixture = TestFixture::setup();

        let bash_tool = BashTool::new(
            fixture.sandbox.clone(),
            fixture.network_whitelist.clone(),
            fixture.policy_engine.clone(),
        );
        let input = ToolInput {
            path: None,
            content: None,
            arguments: vec!["echo".into(), "hello".into()],
            user_role: Role::Viewer,
        };

        let output = bash_tool.execute(&input).await;
        assert!(!output.success, "Viewer 执行命令应该被拒绝");
        assert!(
            output.error.as_ref().unwrap().contains("策略拒绝"),
            "错误信息应包含策略拒绝"
        );
    }

    #[tokio::test]
    async fn test_bash_network_command_blocked() {
        let fixture = TestFixture::setup();

        let bash_tool = BashTool::new(
            fixture.sandbox.clone(),
            fixture.network_whitelist.clone(),
            fixture.policy_engine.clone(),
        );
        let input = ToolInput {
            path: None,
            content: None,
            arguments: vec!["curl".into(), "http://example.com".into()],
            user_role: Role::Admin,
        };

        let output = bash_tool.execute(&input).await;
        assert!(!output.success, "网络命令应该被拦截");
        assert!(
            output.error.as_ref().unwrap().contains("拦截"),
            "错误信息应包含拦截"
        );
    }

    #[tokio::test]
    async fn test_bash_command_output() {
        let fixture = TestFixture::setup();

        let bash_tool = BashTool::new(
            fixture.sandbox.clone(),
            fixture.network_whitelist.clone(),
            fixture.policy_engine.clone(),
        );
        let input = ToolInput {
            path: None,
            content: None,
            arguments: vec!["echo".into(), "-n".into(), "test123".into()],
            user_role: Role::Admin,
        };

        let output = bash_tool.execute(&input).await;
        assert!(output.success, "命令执行应该成功: {:?}", output.error);
        assert!(output.result.contains("test123"));
    }
}

mod git_tool_tests {
    use super::*;

    #[tokio::test]
    async fn test_git_status() {
        let fixture = TestFixture::setup();

        // 初始化 git 仓库
        let _ = std::process::Command::new("git")
            .args(["init", fixture.workspace.to_str().unwrap()])
            .output();

        let git_tool = GitTool::new(fixture.sandbox.clone(), fixture.policy_engine.clone());
        let input = ToolInput {
            path: Some(fixture.workspace.to_str().unwrap().to_string()),
            content: None,
            arguments: vec!["status".into()],
            user_role: Role::Admin,
        };

        let output = git_tool.execute(&input).await;
        // 即使 git 执行失败（环境限制），至少不应该被策略拒绝
        if !output.success {
            assert!(
                !output.error.as_ref().unwrap().contains("策略拒绝"),
                "git status 不应被策略拒绝"
            );
        }
    }

    #[tokio::test]
    async fn test_git_push_denied() {
        let fixture = TestFixture::setup();

        let git_tool = GitTool::new(fixture.sandbox.clone(), fixture.policy_engine.clone());
        let input = ToolInput {
            path: None,
            content: None,
            arguments: vec!["push".into(), "origin".into(), "main".into()],
            user_role: Role::Admin,
        };

        let output = git_tool.execute(&input).await;
        assert!(!output.success, "git push 应该被拒绝");
        assert!(
            output.error.as_ref().unwrap().contains("禁止"),
            "错误信息应包含禁止"
        );
    }

    #[tokio::test]
    async fn test_git_viewer_denied() {
        let fixture = TestFixture::setup();

        let git_tool = GitTool::new(fixture.sandbox.clone(), fixture.policy_engine.clone());
        let input = ToolInput {
            path: None,
            content: None,
            arguments: vec!["status".into()],
            user_role: Role::Viewer,
        };

        let output = git_tool.execute(&input).await;
        assert!(!output.success, "Viewer 执行 git 应该被拒绝");
        assert!(
            output.error.as_ref().unwrap().contains("策略拒绝"),
            "错误信息应包含策略拒绝"
        );
    }
}

// ============ 辅助验证测试 ============

#[test]
fn test_tool_trait_exists() {
    assert!(true);
}

#[test]
fn test_tool_input_structure() {
    let input = ToolInput {
        path: Some("/test".into()),
        content: Some("data".into()),
        arguments: vec!["arg1".into()],
        user_role: Role::Admin,
    };
    assert_eq!(input.path, Some("/test".into()));
    assert_eq!(input.content, Some("data".into()));
    assert_eq!(input.arguments.len(), 1);
}

#[test]
fn test_tool_output_success() {
    let output = ToolOutput::success("result".into());
    assert!(output.success);
    assert_eq!(output.result, "result");
    assert!(output.error.is_none());
}

#[test]
fn test_tool_output_fail() {
    let output = ToolOutput::fail("error".into());
    assert!(!output.success);
    assert!(output.result.is_empty());
    assert_eq!(output.error, Some("error".into()));
}
