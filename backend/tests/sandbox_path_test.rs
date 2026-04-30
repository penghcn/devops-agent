use devops_agent::sandbox::path_check::{PathValidation, PathValidator};

#[test]
fn normal_relative_path_passes() {
    let validator = PathValidator::new("/workspace");
    assert_eq!(validator.validate("src/main.rs"), PathValidation::Ok);
}

#[test]
fn path_traversal_dottedot() {
    let validator = PathValidator::new("/workspace");
    assert_eq!(
        validator.validate("../etc/passwd"),
        PathValidation::TraversalDetected
    );
}

#[test]
fn deep_path_traversal_blocked() {
    let validator = PathValidator::new("/workspace");
    assert_eq!(
        validator.validate("../../../../../etc/shadow"),
        PathValidation::TraversalDetected
    );
}

#[test]
fn sensitive_file_etc_passwd() {
    let validator = PathValidator::new("/workspace");
    assert_eq!(
        validator.validate("/etc/passwd"),
        PathValidation::SensitiveFile
    );
}

#[test]
fn sensitive_ssh_key_blocked() {
    let validator = PathValidator::new("/workspace");
    assert_eq!(
        validator.validate("~/.ssh/id_rsa"),
        PathValidation::SensitiveFile
    );
}

#[test]
fn sensitive_env_file_blocked() {
    let validator = PathValidator::new("/workspace");
    assert_eq!(validator.validate(".env"), PathValidation::SensitiveFile);
}

#[test]
fn url_encoded_traversal_blocked() {
    let validator = PathValidator::new("/workspace");
    assert_eq!(
        validator.validate("..%2f..%2fetc/passwd"),
        PathValidation::TraversalDetected
    );
}
