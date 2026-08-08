//! The loader's behaviour, end to end through the public API.
//!
//! Every test here was ported from the project this crate was extracted out of, with the
//! prefix parameterised to `TEST_`. The ones that describe a Kubernetes mount are the reason
//! the crate exists, and their comments record failures that already happened once.

#![cfg(feature = "loader")]
#![expect(
    clippy::result_large_err,
    reason = "figment::Jail::expect_with fixes the closure's error type to figment::Error"
)]

use secrecy::{ExposeSecret as _, SecretString};
use serde::Deserialize;
use terrace_config::{Error, ShadowPolicy, Terrace};

/// Two levels of nesting, and a `SecretString` leaf — the shape every real secret has.
#[derive(Debug, Deserialize)]
struct Sample {
    auth: Auth,
}

#[derive(Debug, Deserialize)]
struct Auth {
    jwt_secret: SecretString,
}

/// The shape a TOML layer is usually asked to fill: required values, defaulted values, and a
/// whole block that must materialise from its own defaults when nothing mentions it.
#[derive(Debug, Deserialize)]
struct App {
    database: Database,
    #[serde(default)]
    metrics: Metrics,
}

#[derive(Debug, Deserialize)]
struct Database {
    url: SecretString,
    #[serde(default = "default_pool")]
    max_connections: u32,
    #[serde(default = "default_timeout")]
    acquire_timeout_secs: u64,
}

#[derive(Debug, Deserialize)]
struct Metrics {
    #[serde(default = "default_route")]
    route: String,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            route: default_route(),
        }
    }
}

fn default_pool() -> u32 {
    16
}

fn default_timeout() -> u64 {
    10
}

fn default_route() -> String {
    "/metrics".to_owned()
}

/// The loader under test: the `TEST_` dialect, with the two process-level keys reserved the way
/// a real consumer would reserve theirs.
fn layers() -> Terrace {
    Terrace::new("TEST_")
        .reserve("TEST_PROFILE")
        .reserve("TEST_CONFIRM_RESET")
}

fn load<T: serde::de::DeserializeOwned>() -> Result<T, Error> {
    layers().load()
}

/// A jail with no inherited `TEST_*` variables: several of these assertions are about what the
/// environment does *not* contain, and a developer machine with a real value exported would
/// otherwise decide the outcome.
fn jailed(f: impl FnOnce(&mut figment::Jail) -> figment::error::Result<()>) {
    figment::Jail::expect_with(|jail| {
        jail.clear_env();
        f(jail)
    });
}

fn secrets_dir(jail: &figment::Jail) -> std::path::PathBuf {
    jail.directory().join("secrets")
}

/// A symlink, as the error type `figment::Jail` closures return: it has no
/// `From<std::io::Error>`, and `Jail` itself can only create regular files and directories.
#[cfg(unix)]
fn symlink(target: &str, link: &std::path::Path) -> figment::error::Result<()> {
    std::os::unix::fs::symlink(target, link).map_err(|e| {
        figment::Error::from(format!("symlinking {} -> {target}: {e}", link.display()))
    })
}

// ---------------------------------------------------------------------------------------------
// The secrets directory
// ---------------------------------------------------------------------------------------------

#[test]
fn a_secrets_directory_file_supplies_a_nested_key() {
    jailed(|jail| {
        jail.create_dir("secrets")?;
        // A trailing newline, which is what `printf '%s\n'` and every editor produce.
        jail.create_file("secrets/auth__jwt_secret", "s3cret\n")?;
        jail.set_env("TEST_SECRETS_DIR", secrets_dir(jail).display());

        let cfg: Sample = load().map_err(|e| e.to_string()).unwrap();
        assert_eq!(cfg.auth.jwt_secret.expose_secret(), "s3cret");
        Ok(())
    });
}

/// Interior and leading whitespace survive; only trailing line terminators are stripped. A
/// trailing space can be a real character of a real password, so trimming it would corrupt a
/// credential silently rather than fail.
#[test]
fn only_trailing_line_terminators_are_stripped() {
    jailed(|jail| {
        jail.create_dir("secrets")?;
        jail.create_file("secrets/auth__jwt_secret", " pass phrase \r\n\n")?;
        jail.set_env("TEST_SECRETS_DIR", secrets_dir(jail).display());

        let cfg: Sample = load().map_err(|e| e.to_string()).unwrap();
        assert_eq!(cfg.auth.jwt_secret.expose_secret(), " pass phrase ");
        Ok(())
    });
}

/// The layout a Kubernetes `Secret` volume actually has: a `..data` entry and a timestamped
/// directory beside the real keys. Reading either as a key yields a garbage config entry;
/// classifying the real keys with `symlink_metadata` instead of `metadata` yields an empty
/// layer and a service that boots on defaults. Both were live hazards, so this pins the shape
/// rather than the rule.
#[test]
fn a_projected_volume_layout_yields_only_the_real_keys() {
    jailed(|jail| {
        jail.create_dir("secrets")?;
        jail.create_dir("secrets/..2026_08_02_10_00_00")?;
        jail.create_file("secrets/..data", "not a key")?;
        jail.create_dir("secrets/nested")?;
        jail.create_file("secrets/auth__jwt_secret", "from-the-volume")?;
        jail.set_env("TEST_SECRETS_DIR", secrets_dir(jail).display());

        let cfg: Sample = load().map_err(|e| e.to_string()).unwrap();
        assert_eq!(cfg.auth.jwt_secret.expose_secret(), "from-the-volume");
        Ok(())
    });
}

/// The same layout as above, built the way the kubelet actually builds it: the keys are
/// **symlinks** into `..data`, not regular files.
///
/// The test above pins the volume's *names* but writes the keys as real files, and that gap is
/// exactly what let the symlink bug ship — it stayed green while every service in the cluster
/// booted on compiled defaults, because `DirEntry::metadata()` reports a symlink as "not a
/// file" and the whole layer came back empty. Only a real symlink reproduces it.
#[cfg(unix)]
#[test]
fn keys_symlinked_into_dot_data_are_read() {
    jailed(|jail| {
        let data = "..2026_08_02_10_00_00";
        jail.create_dir("secrets")?;
        jail.create_dir(format!("secrets/{data}"))?;
        jail.create_file(
            format!("secrets/{data}/auth__jwt_secret"),
            "from-the-volume",
        )?;

        let dir = secrets_dir(jail);
        symlink(data, &dir.join("..data"))?;
        symlink("..data/auth__jwt_secret", &dir.join("auth__jwt_secret"))?;
        jail.set_env("TEST_SECRETS_DIR", dir.display());

        let cfg: Sample = load().map_err(|e| e.to_string()).unwrap();
        assert_eq!(cfg.auth.jwt_secret.expose_secret(), "from-the-volume");
        Ok(())
    });
}

/// An all-digit secret reaches the config as a string.
///
/// `figment::providers::Env` runs a TOML-ish parse over every value, so the same secret set as
/// `TEST_AUTH__JWT_SECRET=12345678` becomes a `Value::Num`, and figment's default interpreter
/// will not coerce a number back into a string — the boot fails with "invalid type: integer,
/// expected a string". The file layers emit values unparsed precisely so that a numeric
/// password is not a deployment that cannot start.
#[test]
fn an_all_digit_secret_from_a_file_stays_a_string() {
    jailed(|jail| {
        jail.create_dir("secrets")?;
        jail.create_file("secrets/auth__jwt_secret", "12345678")?;
        jail.set_env("TEST_SECRETS_DIR", secrets_dir(jail).display());

        let cfg: Sample = load().map_err(|e| e.to_string()).unwrap();
        assert_eq!(cfg.auth.jwt_secret.expose_secret(), "12345678");
        Ok(())
    });
}

/// A secrets directory the operator named but that is not there fails the boot: they said the
/// secrets were mounted, and booting on defaults instead is the outcome worth avoiding.
#[test]
fn a_missing_secrets_directory_is_fatal() {
    jailed(|jail| {
        jail.set_env(
            "TEST_SECRETS_DIR",
            jail.directory().join("absent").display(),
        );

        let err = load::<Sample>().expect_err("a named directory must exist");
        assert!(err.to_string().contains("TEST_SECRETS_DIR"), "{err}");
        Ok(())
    });
}

#[test]
fn a_dotted_file_name_is_refused_rather_than_nested_wrongly() {
    jailed(|jail| {
        jail.create_dir("secrets")?;
        jail.create_file("secrets/auth.jwt_secret", "x")?;
        jail.set_env("TEST_SECRETS_DIR", secrets_dir(jail).display());

        let err = load::<Sample>().expect_err("`.` is not the nesting separator");
        assert!(err.to_string().contains("__"), "{err}");
        Ok(())
    });
}

// ---------------------------------------------------------------------------------------------
// `_FILE` indirection
// ---------------------------------------------------------------------------------------------

#[test]
fn file_suffix_indirection_supplies_a_key() {
    jailed(|jail| {
        jail.create_file("jwt", "from-the-path")?;
        jail.set_env(
            "TEST_AUTH__JWT_SECRET_FILE",
            jail.directory().join("jwt").display(),
        );

        let cfg: Sample = load().map_err(|e| e.to_string()).unwrap();
        assert_eq!(cfg.auth.jwt_secret.expose_secret(), "from-the-path");
        Ok(())
    });
}

/// A `_FILE` naming a path that cannot be read fails the boot. Skipping it instead is how a
/// secret goes silently unset and the service comes up on a default.
#[test]
fn a_file_suffix_path_that_cannot_be_read_is_fatal() {
    jailed(|jail| {
        jail.set_env(
            "TEST_AUTH__JWT_SECRET_FILE",
            jail.directory().join("absent").display(),
        );

        let err = load::<Sample>().expect_err("an unreadable path must not be skipped");
        let message = err.to_string();
        assert!(
            message.contains("TEST_AUTH__JWT_SECRET_FILE") && message.contains("absent"),
            "the error must name the variable and the path: {message}"
        );
        Ok(())
    });
}

/// `TEST_AUTH__JWT_SECRET_FILE` is what figment's `Env` turns into the unknown key
/// `auth.jwt_secret_file`. If the shadowing check confused that with `auth.jwt_secret` — the key
/// the indirection actually supplies — then every `_FILE` variable would refuse its own value.
#[test]
fn the_indirection_variable_does_not_shadow_the_key_it_supplies() {
    jailed(|jail| {
        jail.create_file("jwt", "fine")?;
        jail.set_env(
            "TEST_AUTH__JWT_SECRET_FILE",
            jail.directory().join("jwt").display(),
        );

        let cfg: Sample = load().map_err(|e| e.to_string()).unwrap();
        assert_eq!(cfg.auth.jwt_secret.expose_secret(), "fine");
        Ok(())
    });
}

/// A reserved key is read before the layered config exists, so a file naming one would be
/// ignored. Ignoring is the silent misconfiguration these layers exist to remove.
#[test]
fn a_reserved_key_cannot_come_from_a_file() {
    jailed(|jail| {
        jail.create_file("profile", "production")?;
        jail.set_env(
            "TEST_PROFILE_FILE",
            jail.directory().join("profile").display(),
        );

        let err = load::<Sample>().expect_err("a reserved key must be refused");
        assert!(err.to_string().contains("TEST_PROFILE"), "{err}");
        Ok(())
    });
}

/// The configuration and secrets-directory variables are reserved without being named, because
/// they are read to decide what the layers *are*.
#[test]
fn the_config_and_secrets_variables_are_reserved_automatically() {
    jailed(|jail| {
        jail.create_file("elsewhere", "/etc/conf.d")?;
        jail.set_env(
            "TEST_CONFIG_FILE",
            jail.directory().join("elsewhere").display(),
        );

        let err = load::<Sample>().expect_err("TEST_CONFIG must be reserved without being named");
        assert!(err.to_string().contains("TEST_CONFIG"), "{err}");
        Ok(())
    });
}

// ---------------------------------------------------------------------------------------------
// The TOML layer
// ---------------------------------------------------------------------------------------------

#[test]
fn env_overrides_and_defaults_apply() {
    jailed(|jail| {
        jail.set_env("TEST_DATABASE__URL", "postgres://localhost/app");
        jail.set_env("TEST_DATABASE__MAX_CONNECTIONS", "32");

        let cfg: App = load().map_err(|e| e.to_string()).unwrap();
        // `SecretString` has no `PartialEq`; comparing requires `expose_secret()`.
        assert_eq!(cfg.database.url.expose_secret(), "postgres://localhost/app");
        assert_eq!(cfg.database.max_connections, 32);
        // Untouched nested default still applies.
        assert_eq!(cfg.database.acquire_timeout_secs, 10);
        // A block untouched by the environment still materialises with its own defaults.
        assert_eq!(cfg.metrics.route, "/metrics");
        Ok(())
    });
}

/// `$TEST_CONFIG` naming a directory merges every `*.toml` in it, later name winning — a
/// `ConfigMap` mounted as a directory of fragments.
#[test]
fn a_toml_directory_merges_its_fragments_in_name_order() {
    jailed(|jail| {
        jail.create_dir("conf.d")?;
        jail.create_file(
            "conf.d/10-base.toml",
            "[database]\nurl = \"postgres://base/app\"\nmax_connections = 5\n",
        )?;
        jail.create_file(
            "conf.d/20-overrides.toml",
            "[database]\nmax_connections = 40\n",
        )?;
        // Neither is a `*.toml`, so neither may contribute; the dot-prefixed one is also what a
        // `ConfigMap` volume puts beside the real keys.
        jail.create_file("conf.d/..data", "[database]\nmax_connections = 1\n")?;
        jail.create_file("conf.d/notes.md", "not config\n")?;
        jail.set_env("TEST_CONFIG", jail.directory().join("conf.d").display());

        let cfg: App = load().map_err(|e| e.to_string()).unwrap();
        assert_eq!(cfg.database.url.expose_secret(), "postgres://base/app");
        assert_eq!(cfg.database.max_connections, 40);
        Ok(())
    });
}

/// The same directory, built the way a `ConfigMap` volume actually is: the fragments are
/// **symlinks** into `..data` rather than regular files.
///
/// The test above writes them as real files, which is why it stayed green while every service
/// in the cluster loaded an empty config layer — `DirEntry::metadata()` does not traverse
/// symlinks and rejected every fragment. Only a real symlink reproduces the mount.
#[cfg(unix)]
#[test]
fn a_configmap_volume_of_symlinked_fragments_is_merged() {
    jailed(|jail| {
        let data = "..2026_08_02_10_00_00";
        jail.create_dir("conf.d")?;
        jail.create_dir(format!("conf.d/{data}"))?;
        jail.create_file(
            format!("conf.d/{data}/config.toml"),
            "[database]\nurl = \"postgres://volume/app\"\nmax_connections = 7\n",
        )?;

        let dir = jail.directory().join("conf.d");
        symlink(data, &dir.join("..data"))?;
        symlink("..data/config.toml", &dir.join("config.toml"))?;
        jail.set_env("TEST_CONFIG", dir.display());

        let cfg: App = load().map_err(|e| e.to_string()).unwrap();
        assert_eq!(cfg.database.url.expose_secret(), "postgres://volume/app");
        assert_eq!(cfg.database.max_connections, 7);
        Ok(())
    });
}

/// A mounted secret outranks the TOML layer, so a `ConfigMap` carrying a placeholder DSN cannot
/// win over the `Secret` that carries the real one.
#[test]
fn a_secrets_directory_outranks_the_toml_layer() {
    jailed(|jail| {
        jail.create_file(
            "config.toml",
            "[database]\nurl = \"postgres://placeholder/app\"\n",
        )?;
        jail.create_dir("secrets")?;
        jail.create_file("secrets/database__url", "postgres://real/app\n")?;
        jail.set_env(
            "TEST_CONFIG",
            jail.directory().join("config.toml").display(),
        );
        jail.set_env("TEST_SECRETS_DIR", secrets_dir(jail).display());

        let cfg: App = load().map_err(|e| e.to_string()).unwrap();
        assert_eq!(cfg.database.url.expose_secret(), "postgres://real/app");
        Ok(())
    });
}

// ---------------------------------------------------------------------------------------------
// Shadowing
// ---------------------------------------------------------------------------------------------

/// A key supplied by both the environment and a mounted file is refused rather than resolved by
/// precedence: the failure being prevented is a stale environment variable shadowing a rotated
/// secret, where the service keeps working on the old credential and the discrepancy surfaces
/// during an incident.
#[test]
fn a_key_supplied_twice_is_refused_and_no_value_is_printed() {
    jailed(|jail| {
        jail.create_dir("secrets")?;
        jail.create_file("secrets/auth__jwt_secret", "from-the-file")?;
        jail.set_env("TEST_SECRETS_DIR", secrets_dir(jail).display());
        jail.set_env("TEST_AUTH__JWT_SECRET", "from-the-environment");

        let err = load::<Sample>().expect_err("a shadowed key must be refused");
        let message = err.to_string();
        assert!(
            message.contains("auth.jwt_secret")
                && message.contains("TEST_AUTH__JWT_SECRET")
                && message.contains("auth__jwt_secret"),
            "the error must name the key and both sources: {message}"
        );
        assert!(
            !message.contains("from-the-file") && !message.contains("from-the-environment"),
            "the error must not print either value: {message}"
        );
        Ok(())
    });
}

#[test]
fn a_key_supplied_by_both_file_mechanisms_is_refused() {
    jailed(|jail| {
        jail.create_dir("secrets")?;
        jail.create_file("secrets/auth__jwt_secret", "a")?;
        jail.create_file("jwt", "b")?;
        jail.set_env("TEST_SECRETS_DIR", secrets_dir(jail).display());
        jail.set_env(
            "TEST_AUTH__JWT_SECRET_FILE",
            jail.directory().join("jwt").display(),
        );

        let err = load::<Sample>().expect_err("a shadowed key must be refused");
        assert!(err.to_string().contains("auth.jwt_secret"), "{err}");
        Ok(())
    });
}

/// `LastWins` exists so the crate is adoptable by anyone already on precedence semantics. The
/// file layer outranks the environment, matching the documented layer order.
#[test]
fn last_wins_lets_a_mounted_file_outrank_the_environment() {
    jailed(|jail| {
        jail.create_dir("secrets")?;
        jail.create_file("secrets/auth__jwt_secret", "from-the-file")?;
        jail.set_env("TEST_SECRETS_DIR", secrets_dir(jail).display());
        jail.set_env("TEST_AUTH__JWT_SECRET", "from-the-environment");

        let cfg: Sample = layers()
            .shadow_policy(ShadowPolicy::LastWins)
            .load()
            .map_err(|e| e.to_string())
            .unwrap();
        assert_eq!(cfg.auth.jwt_secret.expose_secret(), "from-the-file");
        Ok(())
    });
}

/// Between the two file mechanisms, `_FILE` indirection is the later layer and wins.
#[test]
fn last_wins_ranks_indirection_above_the_secrets_directory() {
    jailed(|jail| {
        jail.create_dir("secrets")?;
        jail.create_file("secrets/auth__jwt_secret", "from-the-directory")?;
        jail.create_file("jwt", "from-the-path")?;
        jail.set_env("TEST_SECRETS_DIR", secrets_dir(jail).display());
        jail.set_env(
            "TEST_AUTH__JWT_SECRET_FILE",
            jail.directory().join("jwt").display(),
        );

        let cfg: Sample = layers()
            .shadow_policy(ShadowPolicy::LastWins)
            .load()
            .map_err(|e| e.to_string())
            .unwrap();
        assert_eq!(cfg.auth.jwt_secret.expose_secret(), "from-the-path");
        Ok(())
    });
}

/// `LastWins` relaxes shadowing and nothing else. A file naming a reserved key is still refused,
/// because there is no precedence answer to give: the key is read before the layers exist.
#[test]
fn last_wins_still_refuses_a_reserved_key() {
    jailed(|jail| {
        jail.create_file("profile", "production")?;
        jail.set_env(
            "TEST_PROFILE_FILE",
            jail.directory().join("profile").display(),
        );

        let err = layers()
            .shadow_policy(ShadowPolicy::LastWins)
            .load::<Sample>()
            .expect_err("a reserved key must be refused under either policy");
        assert!(err.to_string().contains("TEST_PROFILE"), "{err}");
        Ok(())
    });
}

// ---------------------------------------------------------------------------------------------
// Watching and the fingerprint
// ---------------------------------------------------------------------------------------------

/// A reload that changes nothing must be detectable as a no-op: a `..data` swap that moved no
/// key would otherwise rebuild the pool and rebind the listener for nothing.
#[test]
fn the_fingerprint_tracks_values_not_reads() {
    jailed(|jail| {
        jail.create_dir("secrets")?;
        jail.create_file("secrets/database__url", "postgres://one/app")?;
        jail.set_env("TEST_SECRETS_DIR", secrets_dir(jail).display());

        let first = layers()
            .load_watched::<App>()
            .map_err(|e| e.to_string())
            .unwrap();
        let again = layers()
            .load_watched::<App>()
            .map_err(|e| e.to_string())
            .unwrap();
        assert!(
            !again.sources.differs_from(&first.sources),
            "re-reading unchanged files must not look like a change"
        );

        jail.create_file("secrets/database__url", "postgres://two/app")?;
        let rotated = layers()
            .load_watched::<App>()
            .map_err(|e| e.to_string())
            .unwrap();
        assert!(
            rotated.sources.differs_from(&first.sources),
            "a rotated secret must look like a change"
        );
        assert!(
            rotated
                .sources
                .watch_paths()
                .iter()
                .any(|p| p.ends_with("secrets")),
            "the secrets directory must be watched: {:?}",
            rotated.sources.watch_paths()
        );
        Ok(())
    });
}

/// The fingerprint is every value merged together, secrets included, so `Debug` must not print
/// it. A `tracing::debug!(?sources)` is otherwise a credential in the log.
#[test]
fn debug_does_not_print_the_fingerprint() {
    jailed(|jail| {
        jail.create_dir("secrets")?;
        jail.create_file("secrets/database__url", "postgres://super-secret/app")?;
        jail.set_env("TEST_SECRETS_DIR", secrets_dir(jail).display());

        let loaded = layers()
            .load_watched::<App>()
            .map_err(|e| e.to_string())
            .unwrap();
        let rendered = format!("{:?}", loaded.sources);
        assert!(
            !rendered.contains("super-secret"),
            "Sources must not print its fingerprint: {rendered}"
        );
        assert!(rendered.contains("secrets"), "{rendered}");
        Ok(())
    });
}

/// The TOML layer's *directory* is watched even when the file does not exist yet: watching a
/// `config.toml` that is not there registers nothing, and a file created later would then never
/// be noticed.
#[test]
fn the_toml_directory_is_watched_even_when_the_file_is_absent() {
    jailed(|jail| {
        jail.create_dir("conf")?;
        let config = jail.directory().join("conf").join("config.toml");
        jail.set_env("TEST_CONFIG", config.display());
        jail.set_env("TEST_DATABASE__URL", "postgres://env/app");

        let loaded = layers()
            .load_watched::<App>()
            .map_err(|e| e.to_string())
            .unwrap();
        assert!(
            loaded
                .sources
                .watch_paths()
                .iter()
                .any(|p| p.ends_with("conf")),
            "the config file's directory must be watched: {:?}",
            loaded.sources.watch_paths()
        );
        Ok(())
    });
}

// ---------------------------------------------------------------------------------------------
// The parameterisation itself
// ---------------------------------------------------------------------------------------------

/// Renamed variables are honoured, and the derived ones are then *not* read — otherwise a
/// deployment that renamed one would silently keep obeying the old name too.
#[test]
fn renamed_variables_replace_the_derived_ones() {
    jailed(|jail| {
        jail.create_dir("vault")?;
        jail.create_file("vault/auth__jwt_secret", "from-the-vault")?;
        jail.create_dir("decoy")?;
        jail.create_file("decoy/auth__jwt_secret", "from-the-decoy")?;
        jail.set_env("APP_VAULT", jail.directory().join("vault").display());
        jail.set_env("TEST_SECRETS_DIR", jail.directory().join("decoy").display());

        let cfg: Sample = layers()
            .secrets_dir_var("APP_VAULT")
            .load()
            .map_err(|e| e.to_string())
            .unwrap();
        assert_eq!(cfg.auth.jwt_secret.expose_secret(), "from-the-vault");
        Ok(())
    });
}

/// A renamed variable is reserved under its new name. The set is resolved at load time, so a
/// [`Terrace::secrets_dir_var`] call after the [`Terrace::reserve`] calls still takes effect.
#[test]
fn a_renamed_variable_is_the_one_that_gets_reserved() {
    jailed(|jail| {
        jail.create_file("path", "/mnt/secrets")?;
        jail.set_env("TEST_VAULT_FILE", jail.directory().join("path").display());

        let err = layers()
            .secrets_dir_var("TEST_VAULT")
            .load::<Sample>()
            .expect_err("TEST_VAULT must be reserved once it is the secrets variable");
        assert!(err.to_string().contains("TEST_VAULT"), "{err}");

        // Under the derived name, `TEST_VAULT` is just another key, and the indirection is
        // honoured rather than refused — it is the rename that reserved it.
        let refused = layers()
            .load::<Sample>()
            .expect_err("auth.jwt_secret is missing");
        assert!(
            !refused.to_string().contains("TEST_VAULT"),
            "TEST_VAULT must not be reserved by default: {refused}"
        );
        Ok(())
    });
}

/// The indirection suffix is a parameter: a deployment spelling it `_PATH` gets `_PATH`, and
/// `_FILE` then means nothing.
#[test]
fn a_custom_indirection_suffix_replaces_the_default() {
    jailed(|jail| {
        jail.create_file("jwt", "from-the-path")?;
        jail.set_env(
            "TEST_AUTH__JWT_SECRET_PATH",
            jail.directory().join("jwt").display(),
        );

        let cfg: Sample = layers()
            .file_suffix("_PATH")
            .load()
            .map_err(|e| e.to_string())
            .unwrap();
        assert_eq!(cfg.auth.jwt_secret.expose_secret(), "from-the-path");
        Ok(())
    });
}

/// The nesting separator is a parameter, and it governs both the environment layer and the
/// secrets-directory file names — they must agree, or a file and a variable would name
/// different fields.
#[test]
fn a_custom_nesting_separator_governs_every_layer() {
    jailed(|jail| {
        jail.create_dir("secrets")?;
        jail.create_file("secrets/auth-jwt_secret", "from-the-file")?;
        jail.set_env("TEST_SECRETS_DIR", secrets_dir(jail).display());

        let cfg: Sample = layers()
            .nesting_separator("-")
            .load()
            .map_err(|e| e.to_string())
            .unwrap();
        assert_eq!(cfg.auth.jwt_secret.expose_secret(), "from-the-file");
        Ok(())
    });
}

/// With no configuration variable set, the TOML layer reads the configured default path.
#[test]
fn the_default_config_path_is_read_when_no_variable_is_set() {
    jailed(|jail| {
        jail.create_file("app.toml", "[database]\nurl = \"postgres://default/app\"\n")?;

        let cfg: App = layers()
            .default_config_path(jail.directory().join("app.toml"))
            .load()
            .map_err(|e| e.to_string())
            .unwrap();
        assert_eq!(cfg.database.url.expose_secret(), "postgres://default/app");
        Ok(())
    });
}
