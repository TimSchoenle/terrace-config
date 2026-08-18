//! The test harness's own tests.
//!
//! Everything in `terrace_config::testing` is load-bearing for every test in every consuming
//! project, and a harness that arranges the wrong thing does not fail — it passes, quietly,
//! against a mount nobody has. So the assertions here are mostly about what ended up on disk and
//! in the environment, rather than about what the loader made of it.

#![cfg(all(feature = "loader", feature = "testing"))]

use std::path::PathBuf;

use terrace_config::testing::{Harness, Layout, Sandbox};
use terrace_config::{Error, Terrace};

/// The loader most of these arrange for.
fn harness() -> Harness {
    Harness::over(Terrace::new("TEST_"))
}

// ---------------------------------------------------------------------------------------------
// The sandbox itself
// ---------------------------------------------------------------------------------------------

/// The whole reason a jail exists: what a test did is gone when it returns, whether or not the
/// next test remembers to undo it.
#[test]
fn the_temporary_directory_and_the_environment_are_both_put_back() {
    let root: PathBuf = harness().run(|jail| {
        jail.env("TEST_HARNESS_MARKER", "set");
        jail.write("a-file", "contents")?;

        assert_eq!(
            std::env::var("TEST_HARNESS_MARKER").as_deref(),
            Ok("set"),
            "a variable set in the jail must be visible inside it"
        );
        Ok(jail.sandbox().root().to_path_buf())
    });

    assert!(!root.exists(), "the sandbox must be deleted: {root:?}");
    assert!(std::env::var_os("TEST_HARNESS_MARKER").is_none());
}

/// A cleared environment is the default because most of what a loader test asserts is about what
/// the environment does *not* hold, and `PATH` is the one variable both CI platforms agree
/// exists.
#[test]
fn the_environment_is_cleared_by_default_and_kept_on_request() {
    harness().run(|_| {
        assert!(std::env::var_os("PATH").is_none());
        Ok(())
    });

    harness().inherit_env().run(|_| {
        assert!(std::env::var_os("PATH").is_some());
        Ok(())
    });
}

/// The working directory is the sandbox, which is what makes a loader left on its default
/// `config.toml` read the one the test wrote.
#[test]
fn the_working_directory_is_the_sandbox() {
    harness().run(|jail| {
        jail.write(
            "config.toml",
            "[database]\nurl = \"postgres://relative/app\"\n",
        )?;

        let figment = jail.figment()?;
        let url: String = figment.extract_inner("database.url").expect("the value");
        assert_eq!(url, "postgres://relative/app");
        Ok(())
    });
}

/// A path that leaves the sandbox is refused rather than written: a file created outside it
/// survives the run, and the next one inherits it.
#[test]
fn a_path_outside_the_sandbox_is_refused() {
    harness().run(|jail| {
        for escape in ["../escape", "nested/../../escape"] {
            let error = jail
                .write(escape, "x")
                .expect_err("a path leaving the sandbox must be refused");
            assert!(error.to_string().contains("sandbox"), "{error}");
        }

        let absolute = jail.path("..").join("escape");
        assert!(jail.write(absolute, "x").is_err());
        Ok(())
    });
}

/// The absolute path every method hands back can be fed straight into the next one.
#[test]
fn an_absolute_path_inside_the_sandbox_is_accepted() {
    harness().run(|jail| {
        let directory = jail.create_dir("conf")?;
        let file = jail.write(directory.join("config.toml"), "x = 1\n")?;

        assert!(file.is_file());
        assert!(file.starts_with(jail.sandbox().root()));
        Ok(())
    });
}

/// The handle a driver task holds writes into the same tree the jail does, and removing through
/// it is visible to the loader.
#[test]
fn the_sandbox_handle_reaches_the_same_tree() {
    harness().run(|jail| {
        jail.secret("auth__jwt_secret", "from-the-jail")?;

        let files: Sandbox = jail.sandbox();
        let moved = std::thread::spawn(move || -> Result<Sandbox, Error> {
            files.write("secrets/auth__jwt_secret", "from-the-thread")?;
            Ok(files)
        })
        .join()
        .expect("the thread did not panic")?;

        let value: String = jail
            .figment()?
            .extract_inner("auth.jwt_secret")
            .expect("set");
        assert_eq!(value, "from-the-thread");

        moved.remove("secrets/auth__jwt_secret")?;
        assert!(
            jail.figment()?
                .extract_inner::<String>("auth.jwt_secret")
                .is_err(),
            "a removed key must be gone from the layer"
        );
        Ok(())
    });
}

// ---------------------------------------------------------------------------------------------
// Reporting a failure
// ---------------------------------------------------------------------------------------------

/// `try_run` hands the error back rather than failing the test, which is what a fuzz oracle and
/// an assertion about a whole arrangement both need.
#[test]
fn try_run_returns_the_error_instead_of_panicking() {
    let outcome: Result<(), Error> =
        harness().try_run(|_| Err(Error::Invalid("deliberate".to_owned())));

    let error = outcome.expect_err("the closure's error must come back");
    assert!(error.to_string().contains("deliberate"), "{error}");
}

#[test]
#[should_panic(expected = "the sandboxed test failed")]
fn run_panics_with_the_error_it_was_given() {
    harness().run(|_| Err::<(), Error>(Error::Invalid("deliberate".to_owned())));
}

// ---------------------------------------------------------------------------------------------
// The names it derives
// ---------------------------------------------------------------------------------------------

/// Every name the harness writes comes from the loader it was built over. A test that spelled
/// these out by hand would keep passing after a rename, against variables nothing reads.
#[test]
fn every_variable_is_derived_from_the_loader_under_test() {
    let loader = Terrace::new("OTHER_")
        .nesting_separator("-")
        .file_suffix("_PATH")
        .config_var("OTHER_SETTINGS")
        .secrets_dir_var("OTHER_VAULT");

    Harness::over(loader).run(|jail| {
        jail.env_key("auth.jwt_secret", "value");
        jail.secret_key("auth.jwt_secret", "value")?;
        jail.indirection("auth.jwt_secret", "value")?;
        jail.config("x = 1\n")?;

        assert!(std::env::var_os("OTHER_AUTH-JWT_SECRET").is_some());
        assert!(std::env::var_os("OTHER_AUTH-JWT_SECRET_PATH").is_some());
        assert!(std::env::var_os("OTHER_SETTINGS").is_some());
        assert!(std::env::var_os("OTHER_VAULT").is_some());
        assert!(jail.path("secrets").join("auth-jwt_secret").is_file());
        Ok(())
    });
}

/// The two halves of a layer are arranged together — the file and the variable that makes the
/// loader read it — for each of the four the loader has.
#[test]
fn each_layer_arranges_its_file_and_its_variable() {
    harness().run(|jail| {
        let secret = jail.secret("auth__jwt_secret", "s3cret")?;
        let fragment = jail.fragment("10-base.toml", "x = 1\n")?;
        let indirect = jail.indirection("auth.other", "value")?;

        assert!(secret.is_file() && fragment.is_file() && indirect.is_file());
        assert_eq!(
            std::env::var("TEST_SECRETS_DIR").as_deref(),
            Ok(jail.path("secrets").to_str().expect("utf-8")),
        );
        assert_eq!(
            std::env::var("TEST_CONFIG").as_deref(),
            Ok(jail.path("conf.d").to_str().expect("utf-8")),
        );
        assert!(std::env::var_os("TEST_AUTH__OTHER_FILE").is_some());
        Ok(())
    });
}

/// A single file and a directory of fragments are the same variable, so the last one arranged is
/// the one in force. Documented, and worth pinning: a test that arranged both would otherwise be
/// asserting about a layer it thinks it replaced.
#[test]
fn the_configuration_variable_follows_the_last_arrangement() {
    harness().run(|jail| {
        jail.config("x = 1\n")?;
        assert_eq!(
            std::env::var("TEST_CONFIG").as_deref(),
            Ok(jail.path("config.toml").to_str().expect("utf-8")),
        );

        jail.fragment("10-base.toml", "x = 2\n")?;
        assert_eq!(
            std::env::var("TEST_CONFIG").as_deref(),
            Ok(jail.path("conf.d").to_str().expect("utf-8")),
        );
        Ok(())
    });
}

// ---------------------------------------------------------------------------------------------
// The volume layouts
// ---------------------------------------------------------------------------------------------

/// The names a projected volume has, which is what the portable layout exists to put on disk.
#[test]
fn the_projected_layout_writes_the_names_a_volume_has() {
    harness().run(|jail| {
        let directory = jail
            .secrets_volume()
            .file("auth__jwt_secret", "from-the-volume")
            .stray_dir("nested")
            .generation("..2026_01_01_00_00_00")
            .projected()
            .create()?;

        assert!(directory.join("..2026_01_01_00_00_00").is_dir());
        assert!(directory.join("..data").is_file());
        assert!(directory.join("nested").is_dir());
        assert!(directory.join("auth__jwt_secret").is_file());

        let value: String = jail
            .figment()?
            .extract_inner("auth.jwt_secret")
            .expect("set");
        assert_eq!(value, "from-the-volume");
        Ok(())
    });
}

/// The layout the kubelet actually writes. The distinction from the one above is the whole
/// reason [`Layout`] is an enum rather than a flag: only a real symlink reproduces the mount
/// that shipped a live incident.
#[cfg(unix)]
#[test]
fn the_symlinked_layout_links_every_key_through_dot_data() {
    harness().run(|jail| {
        let directory = jail
            .secrets_volume()
            .file("auth__jwt_secret", "from-the-volume")
            .symlinked()
            .create()?;

        // `symlink_metadata`, not `metadata`: the whole point of this layout is the distinction
        // between a symlink and what it resolves to, and `metadata` follows the link.
        let data = directory.join("..data");
        let key = directory.join("auth__jwt_secret");
        let link_type = |path: &std::path::Path| {
            std::fs::symlink_metadata(path)
                .expect("the entry exists")
                .file_type()
        };
        assert!(link_type(&data).is_symlink());
        assert!(link_type(&key).is_symlink());
        assert_eq!(
            std::fs::read_link(&key).expect("a symlink"),
            PathBuf::from("..data/auth__jwt_secret"),
            "a key must be linked through `..data`, not straight at the generation"
        );
        assert!(key.is_file(), "the link must resolve to the generation");

        let value: String = jail
            .figment()?
            .extract_inner("auth.jwt_secret")
            .expect("set");
        assert_eq!(value, "from-the-volume");
        Ok(())
    });
}

/// A volume wired to nothing is a decoy: on disk, and pointed at by no variable, which is how a
/// test asserts that a *renamed* variable is the only one being read.
#[test]
fn an_unwired_volume_sets_no_variable() {
    harness().run(|jail| {
        let directory = jail
            .volume("decoy")
            .file("auth__jwt_secret", "from-the-decoy")
            .layout(Layout::Plain)
            .create()?;

        assert!(directory.join("auth__jwt_secret").is_file());
        assert!(std::env::var_os("TEST_SECRETS_DIR").is_none());
        Ok(())
    });
}

// ---------------------------------------------------------------------------------------------
// The supervisor's half
// ---------------------------------------------------------------------------------------------

/// [`Rebuilds`](terrace_config::testing::Rebuilds) has to be able to report *both* answers: that
/// a build arrived, and that one did not. The second is the assertion a supervisor test is
/// usually making, and the one a naive recorder cannot express.
#[cfg(feature = "reload")]
#[test]
fn rebuilds_waits_for_a_build_and_waits_one_out() {
    use std::time::Duration;
    use terrace_config::testing::Rebuilds;

    harness().run(|jail| {
        let rebuilds: Rebuilds = Rebuilds::new().patience(Duration::from_secs(5));

        jail.block_on(async {
            let recorder = rebuilds.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
                recorder.record("first".to_owned());
            });

            rebuilds.wait_for(1).await;
            rebuilds.stays_at(1, Duration::from_millis(100)).await;
        });

        assert_eq!(rebuilds.seen(), ["first"]);
        assert_eq!(rebuilds.count(), 1);
        Ok(())
    });
}
