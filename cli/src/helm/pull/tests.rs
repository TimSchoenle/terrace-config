use serde_json::{Value as Json, json};

use super::{normalized_image, now, reference_for, write_vendored};
use crate::oci::carriers::testing::{
    AMD64, IMAGE, INDEX, Recorded, SIGNER, contract, digest_of, published,
};

// -- what a chart's values say ------------------------------------------------------------

#[test]
fn a_bare_docker_hub_name_is_qualified_before_it_reaches_a_client() {
    // A registry client reads a bare first segment as a hostname and tries to resolve it by
    // DNS; signature verification normalises and it does not, so passing the chart's spelling
    // around would have the two disagree about which image was verified.
    assert_eq!(normalized_image("", "demo/api"), "docker.io/demo/api");
    assert_eq!(normalized_image("ghcr.io", "demo/api"), "ghcr.io/demo/api");
    assert_eq!(normalized_image("", "redis"), "docker.io/library/redis");
}

#[test]
fn a_registry_written_into_the_repository_is_kept_rather_than_put_under_docker_hub() {
    // The helm-charts convention: `registry` stays empty and a non-Docker-Hub repository is
    // written fully qualified, which is what the image template renders and the runtime pulls.
    assert_eq!(normalized_image("", "ghcr.io/org/app"), "ghcr.io/org/app");
    assert_eq!(
        normalized_image("", "registry:5000/app"),
        "registry:5000/app"
    );
    assert_eq!(normalized_image("", "localhost/app"), "localhost/app");
    let image =
        serde_json::json!({"registry": "", "repository": "ghcr.io/org/app", "tag": "v1@sha256:ab"});
    let (normalized, reference, digest) = reference_for(&image, None).expect("it resolves");
    assert_eq!(normalized, "ghcr.io/org/app");
    assert_eq!(reference, "ghcr.io/org/app:v1@sha256:ab");
    assert_eq!(digest.as_deref(), Some("sha256:ab"));
}

#[test]
fn a_digest_pinned_in_the_tag_is_taken_rather_than_resolved() {
    let image = json!({"repository": "demo/api", "tag": format!("v1@{INDEX}")});
    let (repository, reference, digest) =
        reference_for(&image, None).expect("the block names a repository");
    assert_eq!(repository, IMAGE);
    assert_eq!(reference, format!("{IMAGE}:v1@{INDEX}"));
    assert_eq!(digest.as_deref(), Some(INDEX));
}

#[test]
fn a_block_with_no_tag_falls_back_to_the_charts_app_version() {
    let image = json!({"repository": "demo/api"});
    let (_, reference, digest) = reference_for(&image, Some("v2")).expect("it reads");
    assert_eq!(reference, format!("{IMAGE}:v2"));
    assert!(digest.is_none(), "nothing pinned it");
    assert!(reference_for(&json!({}), None).is_err());
}

// -- what is written ----------------------------------------------------------------------

/// A throwaway file, removed when it goes out of scope.
struct Vendored(std::path::PathBuf);

impl Vendored {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let at = std::env::temp_dir().join(format!(
            "terrace-vendored-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&at);
        Self(at)
    }

    fn path(&self) -> std::path::PathBuf {
        self.0.join("contracts").join("api.json")
    }

    fn read(&self) -> Json {
        serde_json::from_str(&std::fs::read_to_string(self.path()).expect("it was written"))
            .expect("it is JSON")
    }
}

impl Drop for Vendored {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_wrapper_records_which_digest_the_bytes_were_fetched_for() {
    // Which is what the offline staleness interlock reads: the tie between a contract and an
    // image is the attachment, and this is where that fact is written down.
    let held = Vendored::new();
    let payload = published();
    let sha256 = crate::oci::hex_sha256(&payload);
    assert!(
        write_vendored(
            &held.path(),
            IMAGE,
            INDEX,
            &payload,
            &sha256,
            "2026-01-01T00:00:00Z"
        )
        .expect("it writes")
    );
    let written = held.read();
    assert_eq!(written["source"]["image"], json!(IMAGE));
    assert_eq!(written["source"]["digest"], json!(INDEX));
    assert_eq!(written["source"]["sha256"], json!(sha256));
    assert_eq!(written["contract"], contract());
}

#[test]
fn a_file_that_differs_only_in_when_it_was_fetched_is_left_alone() {
    // `fetched` moves on every run by construction, and rewriting the file for it alone would
    // put a commit on every scheduled run — noise in exactly the place a real contract change
    // is supposed to stand out.
    let held = Vendored::new();
    let payload = published();
    let sha256 = crate::oci::hex_sha256(&payload);
    let write = |fetched: &str| {
        write_vendored(&held.path(), IMAGE, INDEX, &payload, &sha256, fetched).expect("it writes")
    };
    assert!(write("2026-01-01T00:00:00Z"));
    assert!(!write("2026-06-01T12:00:00Z"));
    assert_eq!(
        held.read()["source"]["fetched"],
        json!("2026-01-01T00:00:00Z")
    );
}

#[test]
fn a_declaration_that_leaves_its_image_to_the_defaults_is_still_refreshed() {
    // The slim declaration names no `images` at all: the image is the one at `image`, and its
    // contract is `contracts/<document>.json`. Reading the raw file found no image here and
    // reported nothing changed, which is a refresh that silently never happens.
    let chart = chart_with(&[
        (super::DECLARATION, "documents:\n  - name: api\n"),
        CHART,
        VALUES,
    ]);
    let registry = Recorded::of(&published());
    let mut refreshed = super::Refreshed::default();
    super::refresh_chart(&chart.0, &registry, SIGNER, "t", &mut refreshed).expect("it refreshes");
    assert_eq!(refreshed.changed, vec![chart.path()]);
    assert_eq!(chart.read()["contract"], contract());
}

/// A chart directory holding the given files, removed with the guard.
fn chart_with(files: &[(&str, &str)]) -> Vendored {
    let chart = Vendored::new();
    std::fs::create_dir_all(&chart.0).expect("the chart directory is created");
    for (name, text) in files {
        std::fs::write(chart.0.join(name), text).expect("the chart file is written");
    }
    chart
}

const CHART: (&str, &str) = ("Chart.yaml", "name: api\nversion: 1.0.0\n");
const VALUES: (&str, &str) = (
    "values.yaml",
    "image:\n  repository: demo/api\n  tag: v1@sha256:aa\n",
);

#[test]
fn a_contract_two_documents_share_is_fetched_once_and_then_left_alone() {
    let declaration = "documents:\n  - name: api\n    images:\n      - values: image\n        \
                           contract: contracts/api.json\n  - name: worker\n    images:\n      \
                           - values: image\n        contract: contracts/api.json\n";
    let chart = chart_with(&[(super::DECLARATION, declaration), CHART, VALUES]);
    let registry = Recorded::of(&published());

    let mut first = super::Refreshed::default();
    super::refresh_chart(&chart.0, &registry, SIGNER, "t", &mut first).expect("it refreshes");
    assert_eq!(first.changed, vec![chart.path()]);
    assert_eq!(
        first.unchanged,
        Vec::<std::path::PathBuf>::new(),
        "the second document names the same file"
    );

    let mut second = super::Refreshed::default();
    super::refresh_chart(&chart.0, &registry, SIGNER, "later", &mut second).expect("it refreshes");
    assert_eq!(second.changed, Vec::<std::path::PathBuf>::new());
    assert_eq!(second.unchanged, vec![chart.path()]);
}

#[test]
fn a_values_path_that_holds_no_image_block_is_refused_by_name() {
    let declaration = "documents:\n  - name: api\n    images:\n      - values: sidecar\n";
    let chart = chart_with(&[(super::DECLARATION, declaration), CHART, VALUES]);
    let registry = Recorded::of(&published());
    let failure = super::refresh_chart(
        &chart.0,
        &registry,
        SIGNER,
        "t",
        &mut super::Refreshed::default(),
    )
    .expect_err("there is no image at `sidecar`");
    assert!(failure.to_string().contains("sidecar"), "{failure}");
    assert_eq!(
        *registry.asked.borrow(),
        Vec::<String>::new(),
        "nothing was fetched"
    );
}

#[test]
fn a_second_document_sharing_a_contract_still_has_its_values_path_checked() {
    let declaration = "documents:\n  - name: api\n    images:\n      - values: image\n        \
                           contract: contracts/api.json\n  - name: worker\n    images:\n      \
                           - values: sidecar\n        contract: contracts/api.json\n";
    let chart = chart_with(&[(super::DECLARATION, declaration), CHART, VALUES]);
    let registry = Recorded::of(&published());
    let failure = super::refresh_chart(
        &chart.0,
        &registry,
        SIGNER,
        "t",
        &mut super::Refreshed::default(),
    )
    .expect_err("there is no image at `sidecar`");
    assert!(failure.to_string().contains("sidecar"), "{failure}");
}

#[test]
fn two_images_vendored_into_one_contract_file_are_refused() {
    let declaration = "documents:\n  - name: api\n    images:\n      - values: image\n        \
                           contract: contracts/api.json\n  - name: worker\n    images:\n      \
                           - values: sidecar\n        contract: ./contracts/api.json\n";
    let values = (
        "values.yaml",
        "image:\n  repository: demo/api\n  tag: v1@sha256:aa\nsidecar:\n  repository: \
             demo/worker\n  tag: v1@sha256:bb\n",
    );
    let chart = chart_with(&[(super::DECLARATION, declaration), CHART, values]);
    let registry = Recorded::of(&published());
    let failure = super::refresh_chart(
        &chart.0,
        &registry,
        SIGNER,
        "t",
        &mut super::Refreshed::default(),
    )
    .expect_err("one file cannot hold two images' contracts");
    let message = failure.to_string();
    assert!(
        message.contains("demo/api") && message.contains("demo/worker"),
        "{message}"
    );
}

#[cfg(unix)]
#[test]
fn a_contracts_link_that_leaves_the_chart_is_refused_before_anything_is_fetched() {
    let declaration = "documents:\n  - name: api\n";
    let chart = chart_with(&[(super::DECLARATION, declaration), CHART, VALUES]);
    let outside = Vendored::new();
    std::fs::create_dir_all(&outside.0).expect("a directory");
    std::fs::write(outside.0.join("api.json"), "untouched\n").expect("a file");
    std::os::unix::fs::symlink(&outside.0, chart.0.join("contracts")).expect("a link");
    let registry = Recorded::of(&published());

    let failure = super::refresh_chart(
        &chart.0,
        &registry,
        SIGNER,
        "t",
        &mut super::Refreshed::default(),
    )
    .expect_err("the link points out of the chart");

    assert!(
        failure.to_string().contains("outside the chart"),
        "{failure}"
    );
    assert_eq!(
        std::fs::read_to_string(outside.0.join("api.json")).expect("readable"),
        "untouched\n"
    );
    assert_eq!(
        *registry.asked.borrow(),
        Vec::<String>::new(),
        "nothing was fetched"
    );
}

#[cfg(unix)]
#[test]
fn a_link_at_the_contract_is_refused_whether_or_not_it_points_anywhere() {
    let declaration = "documents:\n  - name: api\n";
    for (target, existing) in [("planted.json", false), ("values.yaml", true)] {
        let chart = chart_with(&[(super::DECLARATION, declaration), CHART, VALUES]);
        std::fs::create_dir_all(chart.0.join("contracts")).expect("a directory");
        std::os::unix::fs::symlink(chart.0.join(target), chart.path()).expect("a link");
        let registry = Recorded::of(&published());

        let failure = super::refresh_chart(
            &chart.0,
            &registry,
            SIGNER,
            "t",
            &mut super::Refreshed::default(),
        )
        .expect_err("the contract is a link");

        assert!(
            failure.to_string().contains("symbolic link"),
            "{target}: {failure}"
        );
        assert_eq!(
            chart.0.join(target).exists(),
            existing,
            "{target}: nothing was created through the link"
        );
        assert_eq!(
            std::fs::read_to_string(chart.0.join("values.yaml")).expect("readable"),
            VALUES.1,
            "{target}: the values file is untouched"
        );
        assert_eq!(
            *registry.asked.borrow(),
            Vec::<String>::new(),
            "{target}: nothing was fetched"
        );
    }
}

#[cfg(unix)]
#[test]
fn the_write_itself_refuses_a_directory_that_became_a_link_after_the_check() {
    let chart = chart_with(&[CHART]);
    let outside = Vendored::new();
    std::fs::create_dir_all(&outside.0).expect("a directory");
    std::os::unix::fs::symlink(&outside.0, chart.0.join("contracts")).expect("a link");
    let payload = br#"{"version":1}"#;

    let failure = super::write_vendored_in(
        &chart.0,
        &chart.path(),
        IMAGE,
        INDEX,
        payload,
        &digest_of(payload),
        "t",
    )
    .expect_err("the link points out of the chart");

    assert!(
        failure.to_string().contains("outside the chart"),
        "{failure}"
    );
    assert!(!outside.0.join("api.json").exists(), "nothing was written");
}

#[test]
fn a_chart_without_a_declaration_is_not_refreshed() {
    let chart = chart_with(&[CHART, VALUES]);
    let registry = Recorded::of(&published());
    let mut refreshed = super::Refreshed::default();
    super::refresh_chart(&chart.0, &registry, SIGNER, "t", &mut refreshed)
        .expect("there is nothing to do");
    assert_eq!(refreshed.changed, Vec::<std::path::PathBuf>::new());
    assert_eq!(refreshed.unchanged, Vec::<std::path::PathBuf>::new());
    assert_eq!(
        *registry.asked.borrow(),
        Vec::<String>::new(),
        "nothing was fetched"
    );
}

#[test]
fn a_moved_digest_rewrites_the_file() {
    let held = Vendored::new();
    let payload = published();
    let sha256 = crate::oci::hex_sha256(&payload);
    assert!(write_vendored(&held.path(), IMAGE, INDEX, &payload, &sha256, "t").expect("it writes"));
    assert!(write_vendored(&held.path(), IMAGE, AMD64, &payload, &sha256, "t").expect("it writes"));
    assert_eq!(held.read()["source"]["digest"], json!(AMD64));
}

#[test]
fn the_moment_a_refresh_happened_is_spelled_the_way_a_vendored_file_records_it() {
    let held = now();
    assert_eq!(held.len(), 20, "{held}");
    assert!(held.ends_with('Z'), "{held}");
    assert!(
        regex::Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$")
            .expect("it compiles")
            .is_match(&held),
        "{held}"
    );
}

#[test]
fn the_calendar_this_carries_agrees_with_a_known_instant() {
    // Hand-rolled, so it is worth pinning against dates a reader can check: the epoch itself,
    // a leap day, and a year a naive rule would get wrong.
    assert_eq!(super::civil(0), "1970-01-01T00:00:00Z");
    assert_eq!(super::civil(951_782_400), "2000-02-29T00:00:00Z");
    assert_eq!(super::civil(1_767_225_599), "2025-12-31T23:59:59Z");
}
