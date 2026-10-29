<!--
Generated from .github/templates/java-README.md.hbs. Edit that file, not this one.

CI renders it on every pull request and commits the result back to the branch. A push to `main`
whose java/README.md does not match its template fails the `readme` job in
.github/workflows/docs.yml.

The payload comes from TimSchoenle/actions/actions/common/readme-variables, which reads
java/gradle.properties and walks java/docs/. The release tag, the JDK and the documentation table
all come from there, so the release pull request is the commit that corrects them.

Nothing in this comment may contain a mustache that is not a real reference.
-->

# terrace-config (Java)

The Java implementations of the configuration contract at [`spec/`](../spec/README.md): a
five-layer loader, a Spring Boot producer, and a Gradle plugin that keeps a service's contract
current.

[![Release](https://img.shields.io/github/v/release/TimSchoenle/terrace-config?sort=semver&filter=terrace-config-java-v*)](https://github.com/TimSchoenle/terrace-config/releases)
[![CI](https://img.shields.io/github/actions/workflow/status/TimSchoenle/terrace-config/ci.yml?branch=main&label=ci)](https://github.com/TimSchoenle/terrace-config/actions/workflows/ci.yml)
[![Coverage](https://codecov.io/gh/TimSchoenle/terrace-config/graph/badge.svg)](https://codecov.io/gh/TimSchoenle/terrace-config)
[![Licence](https://img.shields.io/github/license/TimSchoenle/terrace-config)](../LICENSE)
[![JDK](https://img.shields.io/badge/JDK-25-blue)](gradle.properties)

## What this is

A service annotates its configuration types, and an annotation processor turns them into
descriptors at compile time. Two producers assemble those descriptors into a contract:
[the vanilla loader](#the-vanilla-loader) and [the Spring Boot producer](#spring-boot). Neither
hand-writes what a document should look like. `terrace-config-spec-tck` holds both to the
conformance corpus the Rust reference implementation authors, so a renamed field fails a test
instead of drifting unnoticed.

## Quick start

Annotate the configuration type and load it:

```java
@TerraceConfig
@Getter
@Setter
public final class Config {
    /** Port the HTTP listener binds to. */
    private int port = 8080;

    /** Where orders are persisted. */
    @Nested
    private Database database = new Database();
}

Config config = TerraceLoader.of("ORDERS_").load(Config.class);
```

Then let the build emit and check the contract:

```kotlin
plugins {
    id("de.timscho.terrace-config") version "terrace-config-java-v0.7.1"
}

terraceContract {
    type = "com.example.Config"
    prefix = "ORDERS_"
}
```

[Installation](#installation) covers the repository and the plugin mapping both blocks need.

## Table of contents

- [Features](#features)
- [Installation](#installation)
- [Usage](#usage)
- [Compatibility](#compatibility)
- [Documentation](#documentation)
- [Contributing](#contributing)

## Features

- **One annotation set, two readers.** Jackson binds the fields and the processor describes them
  from the same `@JsonProperty` renames, so the contract and the binder cannot disagree about a
  key's name. A field the processor cannot describe fails the build, naming the annotation that
  would fix it.
- **The five layers the Rust reference reads**: defaults, TOML, prefixed environment variables, a
  secrets directory and `_FILE` indirection, with the same spellings, shadow policy and error
  messages. `explain()` reports which layer supplied each key without printing a value, and
  `loadWatched` returns what to watch for a reload.
- **Spring Boot without a second loader.** Spring's own binder stays in charge; the module adds the
  `_FILE` layer and documents the one dialect difference, single-underscore nesting.
- **A contract that cannot go stale.** The Gradle plugin emits the contract from the types,
  stamps and conforms it with `terrace-contract`, and fails `check` when the committed copy no
  longer matches.
- **Either Jackson major.** The model deserialises under Jackson 2 and 3, and a codec module per
  major writes the same bytes.

## Installation

The modules are published through [JitPack](https://jitpack.io), built from the release tag. Add
the repository to the build, and to `pluginManagement` with the plugin mapping in
[PUBLISHING.md](docs/PUBLISHING.md#applying-the-gradle-plugin):

```kotlin
repositories {
    mavenCentral()
    maven { url = uri("https://jitpack.io") }
}

dependencies {
    implementation("com.github.TimSchoenle.terrace-config:terrace-config-loader:terrace-config-java-v0.7.1")
    implementation("com.github.TimSchoenle.terrace-config:terrace-config-annotations:terrace-config-java-v0.7.1")
    // The generated descriptors are -core types.
    implementation("com.github.TimSchoenle.terrace-config:terrace-config-core:terrace-config-java-v0.7.1")
    annotationProcessor("com.github.TimSchoenle.terrace-config:terrace-config-processor:terrace-config-java-v0.7.1")
    // Only for the quick start's `@Getter` and `@Setter`; hand-written accessors work as well.
    compileOnly("org.projectlombok:lombok:1.18.48")
    annotationProcessor("org.projectlombok:lombok:1.18.48")
}
```

| Module | What it is | Producer |
|---|---|---|
| `terrace-config-annotations` | `@TerraceConfig` and the rest of the vocabulary a service's types carry. Depends on nothing but the JDK. | |
| `terrace-config-processor` | The annotation processor that generates a descriptor per annotated type. | |
| `terrace-config-core` | The document model, the twelve build-time refusals and contract assembly. Builds no `ObjectMapper`. | |
| `terrace-config-core-jackson2` | The byte-stable `json` codec, on Jackson 2. | |
| `terrace-config-core-jackson3` | The same codec, on Jackson 3. | |
| `terrace-config-loader` | The vanilla five-layer loader, and `ContractMain`, which emits its contract at build time. | `terrace-java`, tier 2 |
| `terrace-config-spring-boot` | The `_FILE` layer over Spring's `Binder`, and `SpringContractMain`, which emits its contract at build time. | `spring-boot`, tier 1 |
| `terrace-config-gradle-plugin` | The `de.timscho.terrace-config` plugin. | |

Three modules are not published. `terrace-config-spec-tck` checks both producers against
[`spec/v1/conformance/`](../spec/v1/conformance/) and runs `terrace-contract conform` over their
output. `terrace-config-example-service` and `terrace-config-example-spring-service` are the same
"orders" service through each producer, mirroring
[`rust/examples/service/`](../rust/examples/service/) field for field. Each has a committed
`contract.json` its build checks and a `Dockerfile` that ships the stamped contract.

## Usage

### The vanilla loader

`TerraceLoader` reads `<PREFIX>CONFIG` (a TOML file or directory), `<PREFIX>SECRETS_DIR`,
`<PREFIX>*` and `<PREFIX><KEY>_FILE`, with `__` as the nesting separator, and binds the merged
result to the type:

```java
Config config = TerraceLoader.of("ORDERS_")
        .reserve("ORDERS_PROFILE")
        .load(Config.class);

Explanation report = TerraceLoader.of("ORDERS_").explain();
```

`ORDERS_DATABASE__URL` sets `database.url`, and `ORDERS_DATABASE__URL_FILE=/run/secrets/url` reads
it from a file. A multi-word field needs `@JsonProperty("snake_case")`, because every layer folds
names to lower case; without one, the key is published as unreachable from the environment.

### Spring Boot

Add `terrace-config-spring-boot`, and `FileIndirectionEnvironmentPostProcessor` registers itself
through `META-INF/spring.factories`. `<NAME>_FILE=/path` then supplies the property `<NAME>` from
the file, ahead of every other property source. Everything else is Spring's own binding.

Spring's relaxed binding nests on a single `_`, where the vanilla loader uses `__`, so
`MYAPP_GITHUB_TOKEN` binds `myapp.github.token`. The contract publishes that dialect as it is.
The plugin needs the Jackson 2 codec on its generator classpath for this producer:

```kotlin
dependencies {
    terraceContractGenerator("com.github.TimSchoenle.terrace-config:terrace-config-core-jackson2:terrace-config-java-v0.7.1")
}
```

### The Gradle plugin

The `de.timscho.terrace-config` plugin emits a service's contract from its types at build time,
stamps it with the build's identity, conforms it with `terrace-contract`, and keeps it equal to the
copy committed beside the service. It applies `java`.

| Task | What it does |
|---|---|
| `terraceContractEmit` | Runs `ContractMain` or `SpringContractMain` on the main runtime classpath (plus `terraceContractGenerator`), on the project's Java toolchain, and writes `build/terrace/contract.unstamped.json`. Cacheable: no stamped value is an input, so a new commit or timestamp never invalidates it. |
| `terraceContract` | `terrace-contract stamp` over that document, then `terrace-contract conform --tier <n>`, written to `build/terrace/contract.json`. A document that does not conform is deleted. |
| `terraceContractCheck` | Fails when the committed `contract.json` is not byte for byte the unstamped emission, naming the first differing line. `check` depends on it. |
| `terraceContractUpdate` | Rewrites the committed `contract.json` with the unstamped emission. |

| `terraceContract { }` property | Default |
|---|---|
| `type`, `prefix` | None; both are required. `type` is the root `@TerraceConfig` type's binary name. |
| `appName` | The project's name. |
| `reserve` | Empty. Each is passed as `--reserve` (`terrace-java` only). |
| `producerArguments` | Empty. Passed to the entry point as they are, for its other options (`--nesting-separator`, `--file-suffix`, `--config-var`, ...). |
| `loader` | `terrace-java` when `terrace-config-loader` is on the runtime classpath, `spring-boot` when `terrace-config-spring-boot` is. Required when both or neither is. |
| `tier` | The producer's claim: 2 for `terrace-java`, 1 for `spring-boot`. |
| `appVersion` | `-PterraceContract.appVersion`, else the project's version unless it is `unspecified`. |
| `revision` | `-PterraceContract.revision`, else `TERRACE_CONTRACT_REVISION`. |
| `created` | `-PterraceContract.created`, else `TERRACE_CONTRACT_CREATED`. Unset by default, so two builds of one commit stamp the same document. |
| `source` | Unset. |
| `executable` | `TERRACE_CONTRACT_BIN` (a relative path resolves against the root project), else `PATH`. No other directory is searched and nothing is downloaded, since the binary that holds a build's contract to the specification is one the build's owner chose. |
| `output` | `build/terrace/contract.json`. |
| `committedContract` | `contract.json` in the project directory. |

The committed copy is compared with the unstamped emission, so it carries no version, commit or
timestamp and changes only when the configuration surface does. Each example's `Dockerfile` shows
the rest of the path: a build stage copies `terrace-contract` from
`ghcr.io/timschoenle/terrace-contract` and runs `terraceContract`, and the runtime stage copies the
stamped contract to `/config/contract.json` under the label block
`terrace-contract render --format dockerfile` prints for it.

## Compatibility

- The modules compile for JDK 25.
- The Gradle plugin compiles for Java 17, the floor Gradle 9 runs on.
- `terrace-config-spring-boot` supports Spring Boot 3 and 4, and follows whichever Jackson major
  the application's Spring Boot BOM resolves.
- The model reads and writes under Jackson 2 and 3; pick the codec module that matches.

## Documentation

| Document | Summary |
|---|---|
| [Building and testing](../java/docs/BUILD.md) | How the Java build is put together: the wrapper and the files that pin versions, the convention plugins in buildSrc, what each test suite proves, and the checks a pull request has to pass. |
| [Jackson 2 and 3](../java/docs/JACKSON.md) | One model serves both Jackson majors, and each major gets its own codec module. This page covers why the two majors can share a classpath, how the model deserialises under either, and what… |
| [Module internals](../java/docs/MODULES.md) | How the Java modules divide the work, class by class: the model and assembly in -core, the annotation processor, the vanilla loader, and the Spring Boot producer. The README covers what to depend on… |
| [Null-safety](../java/docs/NULL_SAFETY.md) | Every library package is @NullMarked, so a reference is non-null unless it is annotated @Nullable. This page covers which annotation library does what, how each is declared, and how the annotations… |
| [Publishing](../java/docs/PUBLISHING.md) | The eight published Java modules are served by JitPack, built from a pushed tag on request. This page covers which modules publish, where their coordinates come from, and how a consumer resolves the… |

## Contributing

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org).

`java/README.md` is generated. Edit `.github/templates/java-README.md.hbs` instead.

The gates a pull request has to pass are in [`.github/workflows/ci.yml`](../.github/workflows/ci.yml),
and all of them run locally. `check` and `terraceContract` need the `terrace-contract` binary; see
[BUILD.md](docs/BUILD.md#the-terrace-contract-binary).

```bash
cd java
TERRACE_CONTRACT_BIN=/path/to/terrace-contract ./gradlew check terraceContract
```

## Security

[SECURITY.md](../SECURITY.md) has the reporting instructions. Do not open a public issue for a
vulnerability.

## Licence

See [LICENSE](../LICENSE) for the terms.
