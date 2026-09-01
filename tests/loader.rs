//! The loader's behaviour, end to end through the public API.
//!
//! Every test here was ported from the project this crate was extracted out of, with the
//! prefix parameterised to `TEST_`. The ones that describe a Kubernetes mount are the reason
//! the crate exists, and their comments record failures that already happened once.
//!
//! They also run through [`terrace_config::testing`], the harness this crate ships for its
//! consumers to write exactly these tests with. Two things follow from that. A test says what it
//! is arranging — a mounted secret, a `ConfigMap` of fragments — rather than which variable
//! spells it, so a rename of the mechanism cannot leave a green test exercising a name the
//! loader no longer reads. And this file is the harness's own first consumer: a change that
//! makes it awkward here makes it awkward everywhere.

#![cfg(all(feature = "loader", feature = "testing"))]

use secrecy::{ExposeSecret as _, SecretString};
use serde::Deserialize;
use terrace_config::testing::Harness;
use terrace_config::{ShadowPolicy, Terrace};

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

/// A sandbox over that loader.
///
/// The environment inside it is empty: several of these assertions are about what the
/// environment does *not* contain, and a developer machine with a real value exported would
/// otherwise decide the outcome. The harness clears it, and puts it back afterwards.
fn harness() -> Harness {
    Harness::over(layers())
}

// ---------------------------------------------------------------------------------------------
// The secrets directory
// ---------------------------------------------------------------------------------------------

#[test]
fn a_secrets_directory_file_supplies_a_nested_key() {
    harness().run(|jail| {
        // A trailing newline, which is what `printf '%s\n'` and every editor produce.
        jail.secret("auth__jwt_secret", "s3cret\n")?;

        let config: Sample = jail.load()?;
        assert_eq!(config.auth.jwt_secret.expose_secret(), "s3cret");
        Ok(())
    });
}

/// Interior and leading whitespace survive; only trailing line terminators are stripped. A
/// trailing space can be a real character of a real password, so trimming it would corrupt a
/// credential silently rather than fail.
#[test]
fn only_trailing_line_terminators_are_stripped() {
    harness().run(|jail| {
        jail.secret("auth__jwt_secret", " pass phrase \r\n\n")?;

        let config: Sample = jail.load()?;
        assert_eq!(config.auth.jwt_secret.expose_secret(), " pass phrase ");
        Ok(())
    });
}

/// The layout a Kubernetes `Secret` volume actually has: a `..data` entry and a timestamped
/// directory beside the real keys, plus a subdirectory. Reading any of them as a key yields a
/// garbage config entry; classifying the real keys with `symlink_metadata` instead of `metadata`
/// yields an empty layer and a service that boots on defaults. Both were live hazards, so this
/// pins the shape rather than the rule.
#[test]
fn a_projected_volume_layout_yields_only_the_real_keys() {
    harness().run(|jail| {
        jail.secrets_volume()
            .file("auth__jwt_secret", "from-the-volume")
            .stray_dir("nested")
            .projected()
            .create()?;

        let config: Sample = jail.load()?;
        assert_eq!(config.auth.jwt_secret.expose_secret(), "from-the-volume");
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
    harness().run(|jail| {
        jail.secrets_volume()
            .file("auth__jwt_secret", "from-the-volume")
            .symlinked()
            .create()?;

        let config: Sample = jail.load()?;
        assert_eq!(config.auth.jwt_secret.expose_secret(), "from-the-volume");
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
    harness().run(|jail| {
        jail.secret("auth__jwt_secret", "12345678")?;

        let config: Sample = jail.load()?;
        assert_eq!(config.auth.jwt_secret.expose_secret(), "12345678");
        Ok(())
    });
}

/// A secrets directory the operator named but that is not there fails the boot: they said the
/// secrets were mounted, and booting on defaults instead is the outcome worth avoiding.
#[test]
fn a_missing_secrets_directory_is_fatal() {
    harness().run(|jail| {
        let absent = jail.path("absent");
        jail.secrets_dir_at(absent);

        let error = jail
            .load::<Sample>()
            .expect_err("a named directory must exist");
        assert!(error.to_string().contains("TEST_SECRETS_DIR"), "{error}");
        Ok(())
    });
}

#[test]
fn a_dotted_file_name_is_refused_rather_than_nested_wrongly() {
    harness().run(|jail| {
        jail.secret("auth.jwt_secret", "x")?;

        let error = jail
            .load::<Sample>()
            .expect_err("`.` is not the nesting separator");
        assert!(error.to_string().contains("__"), "{error}");
        Ok(())
    });
}

// ---------------------------------------------------------------------------------------------
// `_FILE` indirection
// ---------------------------------------------------------------------------------------------

#[test]
fn file_suffix_indirection_supplies_a_key() {
    harness().run(|jail| {
        jail.indirection("auth.jwt_secret", "from-the-path")?;

        let config: Sample = jail.load()?;
        assert_eq!(config.auth.jwt_secret.expose_secret(), "from-the-path");
        Ok(())
    });
}

/// A `_FILE` naming a path that cannot be read fails the boot. Skipping it instead is how a
/// secret goes silently unset and the service comes up on a default.
#[test]
fn a_file_suffix_path_that_cannot_be_read_is_fatal() {
    harness().run(|jail| {
        let absent = jail.path("absent");
        jail.indirection_at("auth.jwt_secret", absent);

        let error = jail
            .load::<Sample>()
            .expect_err("an unreadable path must not be skipped");
        let message = error.to_string();
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
    harness().run(|jail| {
        jail.indirection("auth.jwt_secret", "fine")?;

        let config: Sample = jail.load()?;
        assert_eq!(config.auth.jwt_secret.expose_secret(), "fine");
        Ok(())
    });
}

/// A reserved key is read before the layered config exists, so a file naming one would be
/// ignored. Ignoring is the silent misconfiguration these layers exist to remove.
#[test]
fn a_reserved_key_cannot_come_from_a_file() {
    harness().run(|jail| {
        jail.indirection("profile", "production")?;

        let error = jail
            .load::<Sample>()
            .expect_err("a reserved key must be refused");
        assert!(error.to_string().contains("TEST_PROFILE"), "{error}");
        Ok(())
    });
}

/// The configuration and secrets-directory variables are reserved without being named, because
/// they are read to decide what the layers *are*.
#[test]
fn the_config_and_secrets_variables_are_reserved_automatically() {
    harness().run(|jail| {
        jail.indirection("config", "/etc/conf.d")?;

        let error = jail
            .load::<Sample>()
            .expect_err("TEST_CONFIG must be reserved without being named");
        assert!(error.to_string().contains("TEST_CONFIG"), "{error}");
        Ok(())
    });
}

// ---------------------------------------------------------------------------------------------
// The loader's own variables are mechanism, not configuration
// ---------------------------------------------------------------------------------------------

/// A root that refuses unknown fields, which is what a service declares when a typo in its
/// `ConfigMap` should stop the boot rather than be dropped on the floor.
///
/// Every assertion in this section is invisible without it. A permissive root ignores an unknown
/// key, so the environment layer carrying `config`, `secrets_dir`, `profile` and
/// `strict.value_file` reads as working right up until someone adds `deny_unknown_fields` — and
/// then every one of this loader's own variables is a boot failure naming a field the type was
/// never going to have.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Strict {
    strict: StrictSection,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct StrictSection {
    value: String,
}

/// `$TEST_CONFIG` says where the TOML layer is. It is not a key in it.
#[test]
fn the_config_variable_is_not_offered_to_the_deserialiser() {
    harness().run(|jail| {
        jail.config(
            b"[strict]
value = \"from the file\"
",
        )?;

        let config: Strict = jail.load()?;
        assert_eq!(config.strict.value, "from the file");
        Ok(())
    });
}

/// `$TEST_SECRETS_DIR` says where the mounted `Secret` is. It is not a key in it.
#[test]
fn the_secrets_directory_variable_is_not_offered_to_the_deserialiser() {
    harness().run(|jail| {
        jail.secret_key("strict.value", "from a mounted file")?;

        let config: Strict = jail.load()?;
        assert_eq!(config.strict.value, "from a mounted file");
        Ok(())
    });
}

/// A reserved variable is read before the layers exist and belongs to no layer, so it must not
/// arrive at the deserialiser either — the same reason a file may not supply one.
#[test]
fn a_reserved_variable_is_not_offered_to_the_deserialiser() {
    harness().run(|jail| {
        jail.env("TEST_PROFILE", "production");

        let config: Strict = jail.load()?;
        assert_eq!(config.strict.value, String::new());
        Ok(())
    });
}

/// `TEST_STRICT__VALUE_FILE` names the file that supplies `strict.value`. `strict.value_file` is
/// what figment makes of it, and is a field no type has.
#[test]
fn an_indirection_variable_is_not_offered_to_the_deserialiser() {
    harness().run(|jail| {
        jail.indirection("strict.value", "from the named file")?;

        let config: Strict = jail.load()?;
        assert_eq!(config.strict.value, "from the named file");
        Ok(())
    });
}

// ---------------------------------------------------------------------------------------------
// The TOML layer
// ---------------------------------------------------------------------------------------------

#[test]
fn env_overrides_and_defaults_apply() {
    harness().run(|jail| {
        jail.env_key("database.url", "postgres://localhost/app");
        jail.env_key("database.max_connections", 32);

        let config: App = jail.load()?;
        // `SecretString` has no `PartialEq`; comparing requires `expose_secret()`.
        assert_eq!(
            config.database.url.expose_secret(),
            "postgres://localhost/app"
        );
        assert_eq!(config.database.max_connections, 32);
        // Untouched nested default still applies.
        assert_eq!(config.database.acquire_timeout_secs, 10);
        // A block untouched by the environment still materialises with its own defaults.
        assert_eq!(config.metrics.route, "/metrics");
        Ok(())
    });
}

/// `$TEST_CONFIG` naming a directory merges every `*.toml` in it, later name winning — a
/// `ConfigMap` mounted as a directory of fragments.
#[test]
fn a_toml_directory_merges_its_fragments_in_name_order() {
    harness().run(|jail| {
        jail.config_volume()
            .file(
                "10-base.toml",
                "[database]\nurl = \"postgres://base/app\"\nmax_connections = 5\n",
            )
            .file("20-overrides.toml", "[database]\nmax_connections = 40\n")
            // Neither is a `*.toml`, so neither may contribute; the dot-prefixed one is also
            // what a `ConfigMap` volume puts beside the real keys.
            .stray_file("..data", "[database]\nmax_connections = 1\n")
            .stray_file("notes.md", "not config\n")
            .create()?;

        let config: App = jail.load()?;
        assert_eq!(config.database.url.expose_secret(), "postgres://base/app");
        assert_eq!(config.database.max_connections, 40);
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
    harness().run(|jail| {
        jail.config_volume()
            .file(
                "config.toml",
                "[database]\nurl = \"postgres://volume/app\"\nmax_connections = 7\n",
            )
            .symlinked()
            .create()?;

        let config: App = jail.load()?;
        assert_eq!(config.database.url.expose_secret(), "postgres://volume/app");
        assert_eq!(config.database.max_connections, 7);
        Ok(())
    });
}

/// A mounted secret outranks the TOML layer, so a `ConfigMap` carrying a placeholder DSN cannot
/// win over the `Secret` that carries the real one.
#[test]
fn a_secrets_directory_outranks_the_toml_layer() {
    harness().run(|jail| {
        jail.config("[database]\nurl = \"postgres://placeholder/app\"\n")?;
        jail.secret("database__url", "postgres://real/app\n")?;

        let config: App = jail.load()?;
        assert_eq!(config.database.url.expose_secret(), "postgres://real/app");
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
    harness().run(|jail| {
        jail.secret("auth__jwt_secret", "from-the-file")?;
        jail.env_key("auth.jwt_secret", "from-the-environment");

        let error = jail
            .load::<Sample>()
            .expect_err("a shadowed key must be refused");
        let message = error.to_string();
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
    harness().run(|jail| {
        jail.secret("auth__jwt_secret", "a")?;
        jail.indirection("auth.jwt_secret", "b")?;

        let error = jail
            .load::<Sample>()
            .expect_err("a shadowed key must be refused");
        assert!(error.to_string().contains("auth.jwt_secret"), "{error}");
        Ok(())
    });
}

/// `LastWins` exists so the crate is adoptable by anyone already on precedence semantics. The
/// file layer outranks the environment, matching the documented layer order.
#[test]
fn last_wins_lets_a_mounted_file_outrank_the_environment() {
    harness().run(|jail| {
        jail.secret("auth__jwt_secret", "from-the-file")?;
        jail.env_key("auth.jwt_secret", "from-the-environment");

        let config: Sample = jail
            .terrace()
            .shadow_policy(ShadowPolicy::LastWins)
            .load()?;
        assert_eq!(config.auth.jwt_secret.expose_secret(), "from-the-file");
        Ok(())
    });
}

/// Between the two file mechanisms, `_FILE` indirection is the later layer and wins.
#[test]
fn last_wins_ranks_indirection_above_the_secrets_directory() {
    harness().run(|jail| {
        jail.secret("auth__jwt_secret", "from-the-directory")?;
        jail.indirection("auth.jwt_secret", "from-the-path")?;

        let config: Sample = jail
            .terrace()
            .shadow_policy(ShadowPolicy::LastWins)
            .load()?;
        assert_eq!(config.auth.jwt_secret.expose_secret(), "from-the-path");
        Ok(())
    });
}

/// `LastWins` relaxes shadowing and nothing else. A file naming a reserved key is still refused,
/// because there is no precedence answer to give: the key is read before the layers exist.
#[test]
fn last_wins_still_refuses_a_reserved_key() {
    harness().run(|jail| {
        jail.indirection("profile", "production")?;

        let error = jail
            .terrace()
            .shadow_policy(ShadowPolicy::LastWins)
            .load::<Sample>()
            .expect_err("a reserved key must be refused under either policy");
        assert!(error.to_string().contains("TEST_PROFILE"), "{error}");
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
    harness().run(|jail| {
        jail.secret("database__url", "postgres://one/app")?;

        let first = jail.load_watched::<App>()?;
        let again = jail.load_watched::<App>()?;
        assert!(
            !again.sources.differs_from(&first.sources),
            "re-reading unchanged files must not look like a change"
        );

        jail.secret("database__url", "postgres://two/app")?;
        let rotated = jail.load_watched::<App>()?;
        assert!(
            rotated.sources.differs_from(&first.sources),
            "a rotated secret must look like a change"
        );
        assert!(
            rotated
                .sources
                .watch_paths()
                .iter()
                .any(|path| path.ends_with("secrets")),
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
    harness().run(|jail| {
        jail.secret("database__url", "postgres://super-secret/app")?;

        let loaded = jail.load_watched::<App>()?;
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
    harness().run(|jail| {
        let directory = jail.create_dir("conf")?;
        jail.config_at(directory.join("config.toml"));
        jail.env_key("database.url", "postgres://env/app");

        let loaded = jail.load_watched::<App>()?;
        assert!(
            loaded
                .sources
                .watch_paths()
                .iter()
                .any(|path| path.ends_with("conf")),
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
///
/// Built over the renamed loader rather than renaming inside the jail, so that the volume the
/// harness wires up is the one that loader actually reads. The decoy is wired by name, because
/// after the rename nothing else points at `TEST_SECRETS_DIR`.
#[test]
fn renamed_variables_replace_the_derived_ones() {
    Harness::over(layers().secrets_dir_var("APP_VAULT")).run(|jail| {
        jail.secrets_volume()
            .file("auth__jwt_secret", "from-the-vault")
            .create()?;
        jail.volume("decoy")
            .file("auth__jwt_secret", "from-the-decoy")
            .wire_to("TEST_SECRETS_DIR")
            .create()?;

        let config: Sample = jail.load()?;
        assert_eq!(config.auth.jwt_secret.expose_secret(), "from-the-vault");
        Ok(())
    });
}

/// A renamed variable is reserved under its new name. The set is resolved at load time, so a
/// [`Terrace::secrets_dir_var`] call after the [`Terrace::reserve`] calls still takes effect.
#[test]
fn a_renamed_variable_is_the_one_that_gets_reserved() {
    harness().run(|jail| {
        jail.indirection("vault", "/mnt/secrets")?;

        let error = jail
            .terrace()
            .secrets_dir_var("TEST_VAULT")
            .load::<Sample>()
            .expect_err("TEST_VAULT must be reserved once it is the secrets variable");
        assert!(error.to_string().contains("TEST_VAULT"), "{error}");

        // Under the derived name, `TEST_VAULT` is just another key, and the indirection is
        // honoured rather than refused — it is the rename that reserved it.
        let refused = jail
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
///
/// `jail.indirection` derives the variable from the loader it was built over, so this asserts
/// the rename took effect rather than restating the new spelling — a test naming
/// `TEST_AUTH__JWT_SECRET_PATH` by hand would keep passing if the loader stopped reading it.
#[test]
fn a_custom_indirection_suffix_replaces_the_default() {
    Harness::over(layers().file_suffix("_PATH")).run(|jail| {
        jail.indirection("auth.jwt_secret", "from-the-path")?;
        assert!(std::env::var("TEST_AUTH__JWT_SECRET_PATH").is_ok());
        assert!(std::env::var("TEST_AUTH__JWT_SECRET_FILE").is_err());

        let config: Sample = jail.load()?;
        assert_eq!(config.auth.jwt_secret.expose_secret(), "from-the-path");
        Ok(())
    });
}

/// The nesting separator is a parameter, and it governs both the environment layer and the
/// secrets-directory file names — they must agree, or a file and a variable would name
/// different fields.
#[test]
fn a_custom_nesting_separator_governs_every_layer() {
    Harness::over(layers().nesting_separator("-")).run(|jail| {
        jail.secret_key("auth.jwt_secret", "from-the-file")?;
        assert!(jail.path("secrets").join("auth-jwt_secret").is_file());

        let config: Sample = jail.load()?;
        assert_eq!(config.auth.jwt_secret.expose_secret(), "from-the-file");
        Ok(())
    });
}

/// With no configuration variable set, the TOML layer reads the configured default path.
#[test]
fn the_default_config_path_is_read_when_no_variable_is_set() {
    harness().run(|jail| {
        let path = jail.write("app.toml", "[database]\nurl = \"postgres://default/app\"\n")?;

        let config: App = jail.terrace().default_config_path(path).load()?;
        assert_eq!(
            config.database.url.expose_secret(),
            "postgres://default/app"
        );
        Ok(())
    });
}

/// Found by the `toml_layers` fuzz target. `NaN != NaN`, so a configuration holding a float NaN
/// made its fingerprint unequal to *itself*: every filesystem event looked like a change, and the
/// supervisor tore the service down and rebuilt it for as long as that key stayed in the file.
///
/// A reload loop is worse than any of the failures the fingerprint exists to avoid, and it is
/// permanent — it does not resolve when the volume settles.
#[test]
fn a_configuration_holding_a_nan_is_not_a_perpetual_change() {
    harness().run(|jail| {
        jail.config("[database]\nurl = \"postgres://one/app\"\ntimeout = nan\n")?;

        let first = jail.load_watched::<figment::value::Value>()?;
        let again = jail.load_watched::<figment::value::Value>()?;
        assert!(
            !again.sources.differs_from(&first.sources),
            "a NaN in the configuration made an unchanged file look like a change"
        );

        // And the comparison still notices a real change alongside the NaN.
        jail.config("[database]\nurl = \"postgres://two/app\"\ntimeout = nan\n")?;
        let rotated = jail.load_watched::<figment::value::Value>()?;
        assert!(
            rotated.sources.differs_from(&first.sources),
            "a rotated value alongside a NaN must still look like a change"
        );
        Ok(())
    });
}
