use devops_agent::sandbox::fs_isolation::{FileSystemIsolator, FsIsolationConfig};
use tempfile::tempdir;

fn make_config(tmp: &tempfile::TempDir) -> FsIsolationConfig {
    let ws = tmp.path().join("workspace");
    let t = tmp.path().join("tmp");
    let out = tmp.path().join("output");
    FsIsolationConfig {
        workspace_root: ws,
        tmp_dir: t,
        output_dir: out,
        read_only_mounts: vec![tmp.path().join("readonly")],
    }
}

#[test]
fn fs_isolator_creates_dirs() {
    let tmp = tempdir().unwrap();
    let config = make_config(&tmp);
    let isolator = FileSystemIsolator::new(config);
    isolator.ensure_dirs().unwrap();
    assert!(tmp.path().join("workspace").exists());
    assert!(tmp.path().join("tmp").exists());
    assert!(tmp.path().join("output").exists());
}

#[test]
fn can_read_workspace_file() {
    let tmp = tempdir().unwrap();
    let config = make_config(&tmp);
    let isolator = FileSystemIsolator::new(config);
    assert!(isolator.can_read("src/main.rs"));
}

#[test]
fn cannot_read_outside_workspace() {
    let tmp = tempdir().unwrap();
    let config = make_config(&tmp);
    let isolator = FileSystemIsolator::new(config);
    assert!(!isolator.can_read("../etc/passwd"));
}

#[test]
fn can_write_output_dir() {
    let tmp = tempdir().unwrap();
    let config = make_config(&tmp);
    let isolator = FileSystemIsolator::new(config);
    assert!(isolator.can_write("result.txt"));
}

#[test]
fn cannot_write_workspace_root() {
    let tmp = tempdir().unwrap();
    let config = make_config(&tmp);
    let isolator = FileSystemIsolator::new(config);
    // Write to a path inside workspace (not output) should be denied
    let ws_path = tmp.path().join("workspace/src/main.rs");
    assert!(!isolator.can_write(ws_path.to_str().unwrap()));
}

#[test]
fn cannot_write_traversal_path() {
    let tmp = tempdir().unwrap();
    let config = make_config(&tmp);
    let isolator = FileSystemIsolator::new(config);
    // 未规范化的路径应该被拒绝
    assert!(!isolator.can_write("output/../../../etc/passwd"));
    assert!(!isolator.can_write("../output/../result.txt"));
}

#[test]
fn readonly_mount_allows_read() {
    let tmp = tempdir().unwrap();
    let ro = tmp.path().join("readonly");
    std::fs::create_dir_all(&ro).unwrap();
    let config = FsIsolationConfig {
        workspace_root: tmp.path().join("workspace"),
        tmp_dir: tmp.path().join("tmp"),
        output_dir: tmp.path().join("output"),
        read_only_mounts: vec![ro.clone()],
    };
    let isolator = FileSystemIsolator::new(config);
    assert!(isolator.can_read(
        ro.join("lib.so")
            .to_str()
            .unwrap()
    ));
}
