//! What `Terrace::explain` reports, and — the assertion that matters most — what it never does.
//!
//! Two halves. The provenance tests pin the answer to "which layer supplied this key" for each of
//! the four layers, and for the case where two of them supplied it. The redaction tests pin the
//! property the whole type is built around: no configuration value reaches the report, whichever
//! layer it came from and whichever way the report is rendered.
//!
//! Through [`terrace_config::testing`], as the rest of the suite is, which here buys something
//! specific: a test that arranges `jail.secret_key("auth.jwt_secret", …)` and then asserts the
//! report names a *secrets file* is asserting the mount was read, not merely that some layer
//! produced the value. That is the assertion the harness could not make before this feature.

#![cfg(all(feature = "explain", feature = "testing"))]

use terrace_config::Terrace;
use terrace_config::explain::{Fragment, Layer};
use terrace_config::testing::Harness;

/// The loader under test: the `TEST_` dialect, as the rest of the suite spells it.
fn harness() -> Harness {
    Harness::over(Terrace::new("TEST_").reserve("TEST_PROFILE"))
}

#[test]
fn a_secrets_file_is_named_as_the_source_of_its_key() {
    harness().run(|jail| {
        let path = jail.secret_key("auth.jwt_secret", "value")?;

        let explanation = jail.explain()?;
        let origin = explanation
            .origin("auth.jwt_secret")
            .expect("the mounted key is reported");

        assert_eq!(origin.effective(), &Layer::SecretsFile(path));
        assert!(!origin.is_contested());
        Ok(())
    });
}

#[test]
fn a_toml_key_is_attributed_to_the_fragment_it_is_in() {
    harness().run(|jail| {
        let base = jail.fragment("10-base.toml", "[server]\nport = 8080\n")?;
        let tuning = jail.fragment("20-tuning.toml", "[server]\nworkers = 4\n")?;

        let explanation = jail.explain()?;
        for (key, fragment) in [("server.port", &base), ("server.workers", &tuning)] {
            let origin = explanation.origin(key).expect("the key is reported");
            assert_eq!(
                origin.effective(),
                &Layer::Toml(fragment.clone()),
                "`{key}` is attributed to the wrong fragment"
            );
        }

        // Both fragments read, in the merge order an operator reading the mount would predict.
        let read: Vec<String> = explanation
            .fragments()
            .iter()
            .map(|(path, state)| {
                format!(
                    "{}: {state}",
                    path.file_name().expect("a fragment has a name").display()
                )
            })
            .collect();
        assert_eq!(read, ["10-base.toml: 1 key", "20-tuning.toml: 1 key"]);
        Ok(())
    });
}

/// The report's whole reason for existing: the file is there, the value is not the file's, and
/// nothing in the loader's own output says so. `ShadowPolicy::Reject` refuses the environment
/// against a *mount* — see below — but says nothing about the TOML layer, where a checked-in
/// `config.toml` overridden by a variable is an ordinary, intended override. Ordinary until it is
/// the one you did not know about.
#[test]
fn a_key_two_layers_supply_names_both_of_them() {
    harness().run(|jail| {
        let config = jail.config("[database]\nurl = \"from-the-file\"\n")?;
        jail.env_key("database.url", "from-the-environment");

        let explanation = jail.explain()?;
        let origin = explanation
            .origin("database.url")
            .expect("the key is reported");

        assert!(origin.is_contested());
        assert_eq!(
            origin.sources().cloned().collect::<Vec<_>>(),
            [
                Layer::Toml(config),
                Layer::Env("TEST_DATABASE__URL".to_owned()),
            ],
            "lowest precedence first, and the environment merges over the TOML layer"
        );
        assert_eq!(explanation.contested().count(), 1);
        Ok(())
    });
}

/// A diagnostic that fails for the reason you are running it is not a diagnostic. The loader
/// refuses this pair outright; the report has to answer anyway, and answer with both sources.
#[test]
fn a_configuration_the_loader_refuses_can_still_be_explained() {
    harness().run(|jail| {
        let mounted = jail.secret_key("auth.jwt_secret", "mounted")?;
        jail.env_key("auth.jwt_secret", "stale");

        assert!(
            jail.figment().is_err(),
            "the default shadow policy must refuse this pair"
        );

        let explanation = jail.explain()?;
        let origin = explanation
            .origin("auth.jwt_secret")
            .expect("the contested key is reported");

        assert_eq!(
            origin.sources().cloned().collect::<Vec<_>>(),
            [
                Layer::Env("TEST_AUTH__JWT_SECRET".to_owned()),
                Layer::SecretsFile(mounted),
            ],
            "lowest precedence first, and the mounted file is the one in effect"
        );
        Ok(())
    });
}

#[test]
fn indirection_names_the_variable_that_was_set() {
    harness().run(|jail| {
        let path = jail.indirection("github.token", "value")?;

        let origin = jail
            .explain()?
            .origin("github.token")
            .expect("the key is reported")
            .clone();

        assert_eq!(
            origin.effective(),
            &Layer::Indirection {
                var: "TEST_GITHUB__TOKEN_FILE".to_owned(),
                path,
            }
        );
        Ok(())
    });
}

/// A projected `Secret` volume, which is the mount this crate exists for. The report has to name
/// the per-key entry an operator can `cat`, and must not invent a key out of the `..data` link or
/// the generation directory beside it.
///
/// `projected()` rather than `symlinked()`: the latter is Unix-only, and what is under test here
/// is which *names* become keys, which the portable layout pins exactly as well.
#[test]
fn a_projected_volume_is_reported_by_its_per_key_entries() {
    harness().run(|jail| {
        jail.secrets_volume()
            .projected()
            .file("auth__jwt_secret", "mounted")
            .create()?;

        let explanation = jail.explain()?;
        assert_eq!(
            explanation.origins().len(),
            1,
            "`..data` and the timestamped directory are not keys: {explanation}"
        );

        let origin = explanation
            .origin("auth.jwt_secret")
            .expect("the mounted key is reported");
        assert!(
            matches!(origin.effective(), Layer::SecretsFile(path) if path.ends_with("auth__jwt_secret")),
            "expected the per-key entry, got {}",
            origin.effective()
        );
        Ok(())
    });
}

/// The first thing to check when the TOML layer supplied nothing, and the one thing the path
/// alone cannot tell you.
#[test]
fn a_configured_file_that_is_not_there_is_reported_as_missing() {
    harness().run(|jail| {
        jail.config_at(jail.path("absent.toml"));

        let explanation = jail.explain()?;
        assert!(
            matches!(explanation.fragments(), [(_, Fragment::Missing)]),
            "expected one missing fragment, got {:?}",
            explanation.fragments()
        );
        Ok(())
    });
}

/// The property the type is built around, asserted against every layer at once and against both
/// renderings. A `Display` that leaked would be a credential in a log; a `Debug` that leaked would
/// be a credential in a panic message.
#[test]
fn no_value_from_any_layer_reaches_the_report() {
    const MARKER: &str = "correct-horse-battery-staple";

    harness().run(|jail| {
        jail.config(format!("[database]\nurl = \"{MARKER}-toml\"\n"))?;
        jail.env_key("server.name", format!("{MARKER}-env"));
        jail.secret_key("auth.jwt_secret", format!("{MARKER}-mounted"))?;
        jail.indirection("github.token", format!("{MARKER}-indirect"))?;

        let explanation = jail.explain()?;
        // All four layers really did supply something, or this would assert nothing at all.
        assert_eq!(explanation.origins().len(), 4);

        for rendered in [format!("{explanation}"), format!("{explanation:?}")] {
            assert!(
                !rendered.contains(MARKER),
                "a configuration value reached the report:\n{rendered}"
            );
        }
        Ok(())
    });
}

/// A fragment that will not parse is reported as such, and its contents stay out of the report —
/// which is why no reason is attached: the parser's message quotes the line, and the line can be
/// the credential.
#[test]
fn an_unparseable_fragment_is_reported_without_its_contents() {
    const MARKER: &str = "correct-horse-battery-staple";

    harness().run(|jail| {
        jail.config(format!("url = = \"{MARKER}\"\n"))?;

        let explanation = jail.explain()?;
        assert!(
            matches!(explanation.fragments(), [(_, Fragment::Unreadable)]),
            "expected one unreadable fragment, got {:?}",
            explanation.fragments()
        );
        assert!(!format!("{explanation}").contains(MARKER));
        Ok(())
    });
}

/// The header names every layer, set or not: "`TEST_SECRETS_DIR` unset" is an answer, and it is
/// the answer more often than anything the key table says.
#[test]
fn the_header_names_every_layer_including_the_ones_that_supplied_nothing() {
    harness().run(|jail| {
        // Not `jail.config`, which would set the variable: this is the unset case.
        let rendered = jail.explain()?.to_string();

        for expected in [
            "prefix `TEST_`, 0 keys",
            "TEST_CONFIG unset, default config.toml",
            "environment   TEST_* (none)",
            "secrets dir   TEST_SECRETS_DIR unset",
            "indirection   TEST_*_FILE (none)",
            "none — every value in this configuration is a default",
        ] {
            assert!(
                rendered.contains(expected),
                "expected `{expected}` in:\n{rendered}"
            );
        }
        assert!(!rendered.ends_with('\n'), "a log line adds its own newline");
        Ok(())
    });
}
