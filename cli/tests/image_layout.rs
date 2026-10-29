//! `image verify --image` and `image attach`, through the binary and a real `oras`, against an OCI
//! image layout written here.
//!
//! The registry seam is unit-tested against recorded shapes; what only a real client can show is
//! that the arguments this passes are the ones `oras` reads (`--oci-layout` in particular), and
//! that a document attached by `image attach` is the one `image verify` then finds. A layout needs
//! no registry and no daemon, so this runs anywhere `oras` does.
//!
//! Skipped when `oras` is not installed, except in CI, where a skipped test would be a passing one.

#![cfg(feature = "cli")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::json;
use terrace_contract::oci::hex_sha256;
use terrace_contract::{Contract, DEFAULT_PATH};

/// `oras`, from `ORAS_BIN` or the path; `None` outside CI when it is not installed.
fn oras() -> Option<PathBuf> {
    let found = std::env::var_os("ORAS_BIN").map(PathBuf::from).or_else(|| {
        std::env::split_paths(&std::env::var_os("PATH")?)
            .map(|directory| directory.join("oras"))
            .find(|candidate| candidate.is_file())
    });
    if found.is_none() {
        assert!(
            std::env::var_os("CI").is_none(),
            "oras is not installed, and in CI a skipped test is a passing one: install it or set \
             ORAS_BIN"
        );
        eprintln!("skipped: oras is not installed");
    }
    found
}

/// A scratch directory, removed when it goes out of scope.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let at = std::env::temp_dir().join(format!("terrace-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&at);
        std::fs::create_dir_all(&at).expect("a scratch directory");
        Self(at)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture() -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/api.json"))
        .expect("the fixture")
}

/// Writes one blob into the layout, and returns its descriptor.
fn blob(layout: &Path, media_type: &str, body: &[u8]) -> serde_json::Value {
    let hex = hex_sha256(body);
    std::fs::write(layout.join("blobs/sha256").join(&hex), body).expect("a blob");
    json!({"mediaType": media_type, "digest": format!("sha256:{hex}"), "size": body.len()})
}

/// An OCI image layout holding one single-platform image, tagged `v1`, that carries `labels` and,
/// when `embedded` is given, that document at the contract's default path.
///
/// `nested` puts the image manifest one index down, the shape `docker save` writes: the tag names
/// an index whose only platform is the image. A manifest there is found by digest alone, with no
/// media type in `index.json` to say what it is, which is what a reader has to cope with.
fn layout(
    at: &Path,
    labels: &BTreeMap<&str, String>,
    embedded: Option<&[u8]>,
    nested: bool,
) -> PathBuf {
    let layout = at.join("layout");
    std::fs::create_dir_all(layout.join("blobs/sha256")).expect("the blob store");
    std::fs::write(
        layout.join("oci-layout"),
        r#"{"imageLayoutVersion":"1.0.0"}"#,
    )
    .expect("the layout marker");

    let mut builder = tar::Builder::new(Vec::new());
    if let Some(document) = embedded {
        let mut header = tar::Header::new_gnu();
        header.set_size(document.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, DEFAULT_PATH.trim_start_matches('/'), document)
            .expect("a tar entry");
    }
    let layer_bytes = builder.into_inner().expect("a tar closes");
    let diff_id = format!("sha256:{}", hex_sha256(&layer_bytes));
    let layer = blob(
        &layout,
        "application/vnd.oci.image.layer.v1.tar",
        &layer_bytes,
    );

    let config = serde_json::to_vec(&json!({
        "architecture": "amd64",
        "os": "linux",
        "config": {"Labels": labels},
        "rootfs": {"type": "layers", "diff_ids": [diff_id]},
    }))
    .expect("a config");
    let config = blob(&layout, "application/vnd.oci.image.config.v1+json", &config);

    let manifest = serde_json::to_vec(&json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": config,
        "layers": [layer],
    }))
    .expect("a manifest");
    let mut manifest = blob(
        &layout,
        "application/vnd.oci.image.manifest.v1+json",
        &manifest,
    );
    if nested {
        manifest["platform"] = json!({"architecture": "amd64", "os": "linux"});
        let index = serde_json::to_vec(&json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.index.v1+json",
            "manifests": [manifest],
        }))
        .expect("an index");
        manifest = blob(&layout, "application/vnd.oci.image.index.v1+json", &index);
    }
    manifest["annotations"] = json!({"org.opencontainers.image.ref.name": "v1"});

    std::fs::write(
        layout.join("index.json"),
        serde_json::to_vec(&json!({"schemaVersion": 2, "manifests": [manifest]}))
            .expect("an index"),
    )
    .expect("the index");
    layout
}

fn labels() -> BTreeMap<&'static str, String> {
    let contract = Contract::from_json(&String::from_utf8(fixture()).expect("UTF-8"))
        .expect("the fixture reads");
    contract.labels(DEFAULT_PATH).into_iter().collect()
}

/// The binary, run from `at` so the layout and the document are named relative to it, as a build
/// names them, with `oras` supplied explicitly.
fn run(oras: &Path, at: &Path, args: &[&str], document: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_terrace-contract"))
        .current_dir(at)
        .env("ORAS_BIN", oras)
        .args(args)
        .arg(document)
        .output()
        .expect("the binary runs")
}

#[test]
fn an_image_carrying_the_document_verifies_and_one_carrying_another_does_not() {
    let Some(oras) = oras() else { return };
    let at = Scratch::new("layout-verify");
    layout(&at.0, &labels(), Some(&fixture()), true);
    let verify = ["image", "verify", "--oci-layout", "--image", "layout:v1"];
    let document = Path::new("contract.json");
    std::fs::write(at.0.join(document), fixture()).expect("the document");

    let verified = run(&oras, &at.0, &verify, document);
    assert_eq!(
        verified.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );

    // One byte more is a different file, whatever it parses to.
    let mut other = fixture();
    other.push(b'\n');
    std::fs::write(at.0.join(document), other).expect("the other document");
    let refused = run(&oras, &at.0, &verify, document);
    let said = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(refused.status.code(), Some(1), "{said}");
    assert!(said.contains("not this document byte for byte"), "{said}");
}

#[test]
fn an_image_with_no_carrier_fails_until_the_document_is_attached() {
    let Some(oras) = oras() else { return };
    let at = Scratch::new("layout-attach");
    layout(&at.0, &labels(), None, false);
    // The document in a directory of its own, so `attach` (which hands `oras` the file from its own
    // directory) has to resolve the layout's path against where it was run, not against that.
    std::fs::create_dir_all(at.0.join("out")).expect("the output directory");
    let document = Path::new("out/contract.json");
    std::fs::write(at.0.join(document), fixture()).expect("the document");
    let verify = ["image", "verify", "--oci-layout", "--image", "layout:v1"];

    let bare = run(&oras, &at.0, &verify, document);
    let said = String::from_utf8_lossy(&bare.stderr);
    assert_eq!(bare.status.code(), Some(1), "{said}");
    assert!(said.contains("publishes no document"), "{said}");

    let attach = ["image", "attach", "--oci-layout", "--image", "layout:v1"];
    let attached = run(&oras, &at.0, &attach, document);
    assert_eq!(
        attached.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&attached.stderr)
    );
    let digest = String::from_utf8_lossy(&attached.stdout).trim().to_owned();
    assert!(digest.starts_with("sha256:"), "{digest}");

    let verified = run(&oras, &at.0, &verify, document);
    assert_eq!(
        verified.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );

    // Again: the same referrer, not a second one.
    let again = run(&oras, &at.0, &attach, document);
    assert_eq!(again.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&again.stdout).trim(), digest);
    assert!(String::from_utf8_lossy(&again.stderr).contains("already carries"));

    // A different document is refused rather than attached beside the first.
    let mut other = fixture();
    other.push(b'\n');
    std::fs::write(at.0.join(document), other).expect("the other document");
    let refused = run(&oras, &at.0, &attach, document);
    let said = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(refused.status.code(), Some(2), "{said}");
    assert!(
        said.contains("already carries a different contract"),
        "{said}"
    );
}

#[test]
fn a_layout_cannot_be_held_to_a_signature() {
    let output = Command::new(env!("CARGO_BIN_EXE_terrace-contract"))
        .args([
            "image",
            "verify",
            "--oci-layout",
            "--image",
            "layout:v1",
            "--signer",
            "x",
            "contract.json",
        ])
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be used with"));
}
