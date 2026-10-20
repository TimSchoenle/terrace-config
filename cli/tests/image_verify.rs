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

/// `render --format dockerfile --write` fills the regions, and `--check` then passes on them.
#[test]
fn a_written_dockerfile_is_one_check_accepts() {
    let at = std::env::temp_dir().join(format!("terrace-dockerfile-write-{}", std::process::id()));
    std::fs::create_dir_all(&at).expect("scratch dir");
    let dockerfile = at.join("Dockerfile");
    std::fs::write(
        &dockerfile,
        "FROM scratch\n# terrace-config:labels:begin\n# terrace-config:labels:end\n",
    )
    .expect("dockerfile");
    let contract = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/api.json");
    let render = |mode: &str| {
        Command::new(env!("CARGO_BIN_EXE_terrace-contract"))
            .args(["render", "--format", "dockerfile", mode])
            .arg(&dockerfile)
            .arg(&contract)
            .output()
            .expect("the binary runs")
    };

    let stale = render("--check");
    let written = render("--write");
    let current = render("--check");
    let refused = Command::new(env!("CARGO_BIN_EXE_terrace-contract"))
        .args(["render", "--format", "toml", "--write"])
        .arg(&dockerfile)
        .arg(&contract)
        .output()
        .expect("the binary runs");
    let _ = std::fs::remove_dir_all(&at);

    assert_eq!(stale.status.code(), Some(1), "an empty region is stale");
    assert_eq!(written.status.code(), Some(0));
    assert_eq!(
        current.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&current.stderr)
    );
    assert_eq!(
        refused.status.code(),
        Some(2),
        "--write needs --format dockerfile"
    );
}
