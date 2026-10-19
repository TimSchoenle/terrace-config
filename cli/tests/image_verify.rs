//! `image verify --dockerfile`, through the binary: what a pipeline sees when the file is refused.

#![cfg(feature = "cli")]

use std::path::Path;
use std::process::Command;

#[test]
fn a_refused_dockerfile_is_named_in_the_error() {
    let at = std::env::temp_dir().join(format!("terrace-image-verify-{}", std::process::id()));
    std::fs::create_dir_all(&at).expect("scratch dir");
    let dockerfile = at.join("Dockerfile");
    std::fs::write(&dockerfile, "FROM scratch\n").expect("dockerfile");
    let contract = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/api.json");

    let output = Command::new(env!("CARGO_BIN_EXE_terrace-contract"))
        .args(["image", "verify", "--dockerfile"])
        .arg(&dockerfile)
        .arg(&contract)
        .output()
        .expect("the binary runs");
    let _ = std::fs::remove_dir_all(&at);

    assert_eq!(output.status.code(), Some(2));
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(
        said.starts_with(&format!("{}: ", dockerfile.display())),
        "{said}"
    );
}
