# Module internals

How the Java modules divide the work, class by class: the model and assembly in `-core`, the
annotation processor, the vanilla loader, and the Spring Boot producer. The README covers what to
depend on and how to use it.

## Contents

- [The shared pipeline](#the-shared-pipeline)
- [`terrace-config-core`](#terrace-config-core)
- [`terrace-config-annotations` and `terrace-config-processor`](#terrace-config-annotations-and-terrace-config-processor)
- [`terrace-config-loader`](#terrace-config-loader)
- [`terrace-config-spring-boot`](#terrace-config-spring-boot)

## The shared pipeline

Both producers run the same three steps, and only the dialect differs:

1. The processor reads a `@TerraceConfig` type at compile time and generates a dialect-agnostic
   `TypeDescriptor`.
2. `SchemaAssembler` combines the descriptor with a producer's `Dialect` into a `Schema`.
3. `ContractAssembler` combines the `Schema` with an `App` and a `Producer` into a validated
   `Contract`.

`-core` knows about documents, not binders: neither Spring nor the loader's layers appear in it.
That is what lets `-loader` and `-spring-boot` share steps 2 and 3, and lets `-spec-tck` compare
their output against the corpus with no producer of its own.

## `terrace-config-core`

The document model (`de.timscho.config.core.model`) and the twelve build-time refusals
(`de.timscho.config.core.refusal`) mirror
[`spec/v1/contract.schema.json`](../../spec/v1/contract.schema.json) field for field. The module
builds no `ObjectMapper`; see [JACKSON.md](JACKSON.md) for the codecs.

- **The model** is one Lombok `@Value @Builder @Jacksonized` class per meta-schema object
  (`Producer`, `App`, `Schema`, `Dialect`, `LoaderVar`, `Key`, `External`, `ExternalVar`), four
  enums for the closed vocabularies (`LoaderRole`, `TextForm`, `UnreachableReason`,
  `ExternalUnknownPolicy`), and an open, order-preserving `JsonSchemaDocument` for the
  `json_schema` half, which has no closed shape. Every class carries an explicit
  `@JsonPropertyOrder` and a `@JsonProperty` per field, because a class-level `@JsonNaming` is
  understood only by Jackson 2's `databind`. `Key` and `ExternalVar` set `@JsonInclude` per field:
  the meta-schema's `required` list and its open properties list do not coincide, so inclusion is
  decided field by field. `constraint` is optional on both; `Key.ty` is required but nullable, and
  `ExternalVar.ty` is optional and omitted when null.
- **`ContractValidator.validate(Contract)`** runs `spec/v1/FORMAT.md`'s "What a producer MUST
  refuse" and throws the first violation as its own `ContractRefusalException` subclass. Both
  producers build the same `Contract` shape and must refuse the same twelve things, so it is
  written once here.
- **`Schema.refine(path, Refinement)` and `refineWith(at, Refine)`** publish a constraint a type
  cannot state, with the Rust crate's rules and messages:
  - `Refinement.requiredEntries(...)` names a map's required entries, and
    `Refinement.entryNames(...)` the pattern its entry names must match. An entry-name pattern
    raises the schema to `schema_version: 3`.
  - `Refinement.holds(condition)` relates a struct's fields (exactly one of, when-then, present,
    empty, equal, above, matches), published as a described `allOf` member at `schema_version: 4`.
  - `Refinement.pattern(...)` and `Refinement.nonBlank()` hold a string to a pattern.

  A path continues inside a key's constraint, `*` for an element and a field name for a field,
  wherever the constraint describes one. An unknown path, a key that is not an open map, an entry
  name some layer could not spell, a pattern outside the portable subset (`PortablePattern`), or a
  required entry the pattern rejects is a `RefinementException`. A default the refinement rejects
  makes the key required with no default, whichever of `refine` and `withDefaultsFromValue` runs
  first. A library implements `Refine` with paths relative to wherever a host mounts it.
- **`Schema.toJsonSchemaWith(JsonSchemaOptions)`** renders a `Schema` as the JSON Schema half a
  `Contract` embeds under `json_schema`, porting the Rust crate's `schema::json_schema` module.
  `Node` groups the flat, dotted `Key.path` list into the nested tree a `properties` object needs.
  Each leaf copies the key's own `constraint` and adds `description`, `default` and `writeOnly`.
  `JsonSchemaOptions.forContract()` is draft-07, closed, with `requirePresent` off: the options the
  Rust crate embeds with. Every returned map is a `TreeMap`, so its order is alphabetical, matching
  `serde_json`'s default map. `JsonSchemaRendererCorpusTest`, in `-core-jackson2` because it needs
  an `ObjectMapper` to parse the corpus, re-renders each stored case's `schema` and asserts it
  equals that case's stored `json_schema`.
- **`SchemaAssembler`** (`de.timscho.config.core.descriptor`) combines a `TypeDescriptor` with a
  `Dialect` into a `Schema`, the Java equivalent of `Schema::describe_at`.
  - A `@Nested` field opens a level instead of becoming a key.
  - A container field (`List`, `Set`, `Map`, nested-struct elements included) stays one
    `Structured` key, since an environment variable cannot address an index inside itself. Its
    `constraint` carries the shape of one element, as `rust_type::interpret_with` does: `items`
    for a `List` or `Set`, `additionalProperties` for a `Map`. A scalar element contributes its own
    type. An `@Element` struct contributes its keys as nested `properties` with `required`, closed
    exactly where the struct carries `@JsonIgnoreProperties(ignoreUnknown = false)`, and composed
    as deep as element structs hold containers of their own (`JsonSchemaRenderer.elementObject`).
  - It ports `env_spelling` and `secrets_file_name`. An environment name a case-folding,
    separator-splitting reader cannot map back to the same path is `Unreachable.Unnameable`; a name
    colliding with the indirection suffix is `Unreachable.Indirection`; a reserved key's file
    spellings are cleared.
  - `Key.required` is `false` when the field is `Optional` or `KeyDescriptor.hasDefault()` is set.
    The processor decides the latter from the field's declaration, the Java analogue of the Rust
    derive macro's syntactic check, so neither language needs a live instance to know a key is
    optional.
- **`Schema.withDefaultsFromValue(Map<String, Object>)`** fills in each non-required key's
  `defaultText` and `defaultValue` from an already-nested map, such as
  `objectMapper.convertValue(instance, Map.class)` in a caller that has Jackson. A `Key.secret` key
  is redacted to `<redacted>` after rendering, so an unset secret is not hidden. The marker does not
  make the contract emittable: `ContractValidator` refuses a secret key with any default, so a
  secret field must not have one. A required key is left untouched.
- **Every other rendering is `terrace-contract render`'s.** The Markdown tables, the commented
  `config.toml`, the standalone JSON Schema, the image labels and the Dockerfile marker block are
  read off the emitted document by `terrace-contract render`, and `terrace-contract image verify`
  checks an image against its contract. A second implementation of a rendering could only drift
  from the first.
- **`ContractAssembler`** (`de.timscho.config.core.contract`) is the Java equivalent of
  `Schema::into_contract` followed by `ContractBuilder::build`. It renders `json_schema` with
  `JsonSchemaOptions.forContract()`, derives a declared `ExternalVar`'s `constraint` from its `ty`
  and `values` when the caller left it unset, and runs `ContractValidator`. `ProducerIdentity`
  supplies the one `Producer` this module may publish; `name` and `version` are not
  caller-settable, for the reason the Rust crate's `Producer::current` gives.
- **`ProducerArguments`, `ProducerCommandLine` and `ConfigurationType`** are what the two
  build-time entry points share:
  - a `--name value` parser with no dependency, because the entry points run on a service's own
    classpath, where every library they brought would be one more version to reconcile;
  - `--out`, `--help`, and the exit statuses `terrace-contract` uses: `0` written, `1` refused, `2`
    a usage or classpath error;
  - the generated descriptor and a default-constructed instance of a type named by its binary
    name. `ConfigurationType` loads the type without initialising it, and the descriptor is always
    the binary name plus `Descriptor` (see `DescriptorNaming` below).

## `terrace-config-annotations` and `terrace-config-processor`

The Java answer to `#[derive(Describe)]` (see
[`rust/docs/SCHEMA.md`](../../rust/docs/SCHEMA.md)). A service annotates its configuration types,
and the JSR-269 processor generates a sibling `<Type>Descriptor` class per annotated type, exposing
`public static final TypeDescriptor DESCRIPTOR`. The descriptor records
(`TypeDescriptor`, `KeyDescriptor`, `ElementDescriptor`, `RangeConstraint`) live in
`de.timscho.config.core.descriptor`. A descriptor is dialect-agnostic, with no environment
spelling, alias or `text_form`, which is what keeps the processor usable by both producers.

- **The vocabulary.** `@TerraceConfig` marks a class or record whose fields become keys, or an
  enum whose constants become the values one key accepts. `@Nested` recurses into a field's own
  `@TerraceConfig` type. `@Values`, `@Values(from = ...)` and `@Values({"a", "b"})` report a
  field's accepted spellings (bare, from a mirror enum, or a literal list), the three forms
  `SCHEMA.md`'s `values`, `values_from` and `values(...)` draw. `@Range` bounds a numeric field.
  `@Note`, `@Secret` and `@Skip` match their Rust equivalents. `@Element` and `@ElementValues` ask
  the `@Nested` and `@Values` questions one level into a container's element (`Optional`, `List`,
  `Set`, `Map`, found by unwrapping generics). A map's key type is skipped, since a TOML table's
  keys are strings whatever the map is keyed by.
- **The compile error is the feature.** A field whose type is none of the recognised leaves
  (`String`, the primitives and their boxes, `BigInteger`, `BigDecimal`) and carries no resolving
  annotation fails the build, naming the field, its type, and the attributes that would resolve
  it. The same diagnostic covers a container whose element publishes nothing, a `@Range` on a
  non-numeric type or with no bound, a `@Nested` or `@Values` pointed at a type not annotated the
  right way, and more than one shape annotation on one field.
- **Jackson is read, not duplicated.** A struct's `closed` flag comes from
  `@JsonIgnoreProperties(ignoreUnknown = false)`, an enum constant's spelling from `@JsonProperty`,
  and a field's key path follows its `@JsonProperty` rename, matching whichever binder reads that
  name (`FieldResolver.resolve`). All of them are read through `AnnotationMirror` by qualified
  name, so the processor takes no compile dependency on either Jackson major.
- **A field's initializer decides `KeyDescriptor.hasDefault`.** `VariableElement` answers "does
  this field have a constant value" only for a `static final` field. An instance field such as
  `private String port = "8080";` needs the compiler's `Trees` API (`com.sun.source.util.Trees`) to
  see whether its declaration has an initializer. `jdk.compiler` exports that package
  unconditionally, so no `--add-exports` is needed. `FieldResolver` reads it once per field and
  bakes it into the generated `KeyDescriptor`.
- **Generated descriptors are top-level classes**, even for a nested `@TerraceConfig` type.
  `Filer.createSourceFile` treats every dot in a name as a package separator, so `Outer.Inner`
  cannot become `Outer.InnerDescriptor`. `DescriptorNaming` joins every enclosing simple name with
  `$` (`Outer$InnerDescriptor`) in the type's own package, and both the processor and field
  resolution use it, so references between two generated descriptors compile.

## `terrace-config-loader`

The vanilla five-layer loader, `de.timscho.config.loader`: the Java equivalent of the Rust crate's
`Terrace`, `Dialect`, `provider::*` and `layers::*`, ported class for class, since the layering is
the product.

- **`Dialect`** carries the prefix, the nesting separator (`__`), the `_FILE` suffix and the
  reserved keys a file may never supply. It is immutable with `with`-style methods. `keyPath` and
  `envSpelling` convert a key between its two spellings; separator and reserved-key matching are
  case-insensitive, for the Windows-environment and mixed-case-file reasons the Rust doc comments
  give.
- **The layers.** `TomlLayers` reads one file, or every `*.toml` in a directory, sorted and
  deep-merged. `SecretsDir` reads a directory of key-named files, skipping dotfiles and
  non-regular entries and following symlinks so a Kubernetes projected `Secret` volume works.
  `FileSuffixEnv` resolves `<PREFIX><KEY>_FILE=/path` indirection by scanning, since the keys are
  open-ended. Each produces a nested `Map<String, Object>`. `LayerValues` holds the shared helpers:
  `insertNested`, `deepMerge`, and `readValue`, which is strict UTF-8 and trims a trailing `\r` or
  `\n` but never spaces.
- **TOML conversion.** `tomlj`'s `TomlTable.toMap()` leaves a nested table as a live `TomlTable`,
  which Jackson would read through bean introspection and report as a spurious `"empty"` property.
  `TomlLayers` converts `TomlTable` and `TomlArray` recursively through `keySet()` and `get()`
  instead.
- **`ShadowPolicy`** (`REJECT`, the default, or `LAST_WINS`) and **`FileLayers`**, which collects
  the secrets-directory and indirection layers so the shadow check sees both, are ported from
  `layers.rs`, error messages included. Under `REJECT`, a key supplied by more than one of the
  environment, the secrets directory and indirection is refused.
- **`TerraceLoader`** is the builder: `configVar`, `secretsDirVar`, `defaultConfigPath`,
  `fileSuffix`, `nestingSeparator`, `reserve` and `shadowPolicy`, then `load(Class<T>)`. Layers
  merge lowest precedence first: TOML, then prefixed environment variables (excluding reserved
  names and `_FILE` indirections), then the file-backed layers. The merged map is bound to the
  caller's type with Jackson's `ObjectMapper.convertValue`, an internal detail. `load(Class<T>,
  Map<String, String>)` takes the environment as a map instead of `System.getenv()`.
- **`schema(descriptor)` and `schemaAt(descriptor, root)`** describe every key a `TypeDescriptor` can carry,
  spelled in this loader's dialect, mirroring `Terrace::schema` and `schema_at`. They supply the
  loader's dialect to `SchemaAssembler`, plus the loader-read variables (`config`, `secrets_dir`
  and each reserved key) as `LoaderVar`s, which no descriptor can see.
- **`explain()`** reports which layer supplied every key without binding to a type, ported from
  `explain.rs`. `Layer` is a sealed interface (`Toml`, `Env`, `SecretsFile`, `Indirection`), which
  gives the Rust `enum`'s exhaustiveness at compile time. `Origin` names the effective layer plus
  every layer that lost, lowest precedence first. `Explanation.toString()` renders the same report
  as the Rust `Display` impl and carries no configuration value, only paths and variable names, so
  logging one is safe. One package-private `TerraceLoader.Layers` record holds each layer's
  unmerged read for both `load` and `explain`, so the two cannot disagree about what was read.
- **`loadWatched(Class)`** returns a `Loaded<T>`: the bound value plus a `Sources`, as
  `Terrace::load_watched` does. `watchPaths()` collects the parent directory of every file-backed
  input, not the file, because a Kubernetes volume update renames a new `..data` directory over
  the old one and a watch on the old inode never fires again. `Sources.differsFrom` compares the
  merged map structurally, comparing `Double` and `Float` by raw bits so a configuration holding
  `NaN` equals itself. `Sources.toString()` never prints the fingerprint, which holds every value,
  secrets included.
- **`ContractMain`** emits a service's contract at build time without starting it. Given the root
  type's binary name, the prefix and `app.name`, it builds the schema with the `TerraceLoader`
  settings the service boots with (`--reserve`, repeatable, `--nesting-separator`, `--file-suffix`,
  `--config-var`, `--secrets-dir-var`, `--default-config-path`), fills in defaults from a
  default-constructed instance converted through Jackson, and writes the contract to `--out` or
  standard output. Build identity (`app.version`, `revision`, `created`, `source`) is not an
  option: it is `terrace-contract stamp`'s, so the emission depends only on the types and the
  defaults their initializers and constructors produce. It can be compared with a committed copy
  as long as those defaults are deterministic: one read from the clock or the environment changes
  the contract between builds. The Gradle plugin runs it; by hand:

  ```bash
  java -cp <runtime classpath> de.timscho.config.loader.ContractMain \
      --type com.example.Config --prefix MYAPP_ --app-name my-service \
      --reserve MYAPP_PROFILE --out build/terrace/contract.unstamped.json
  ```

## `terrace-config-spring-boot`

The Spring Boot producer, `de.timscho.config.spring`. Spring's `Binder` and
`ConfigDataEnvironmentPostProcessor` already provide files, profiles and relaxed environment
binding, so this module adds only what Spring lacks.

- **`FileIndirectionEnvironmentPostProcessor`** adds the `_FILE` layer: `<NAME>_FILE=/path` names a
  file whose contents supply the property `<NAME>` would otherwise name, the convention a mounted
  Kubernetes `Secret` or Docker secret is addressed by. It runs once, before any bean exists,
  scanning the `systemEnvironment` property source for `_FILE` names and reading each file (strict
  UTF-8, trailing `\r` or `\n` trimmed, never spaces). The results go into a property source at
  the front of the environment, so a mounted secret outranks an environment variable or a
  configuration file, the same precedence as the loader's `FileLayers`. It is registered through
  `META-INF/spring.factories`, because it has to run before `AutoConfiguration.imports` is read.
- **`SpringDialect`** names the one divergence from the loader's `Dialect`. Spring's relaxed
  binding treats a single `_` as the environment's nesting separator (`MYAPP_GITHUB_TOKEN` binds
  `myapp.github.token`), where the loader uses `__` so a field name may contain an underscore.
  Changing Spring's convention would break every property it already binds, so the ambiguity is
  permanent for this producer. `SpringDialectTest` documents and exercises it.
- **`SpringContractProducer.produce`** builds a `Dialect` from `SpringDialect` (same prefix, `_`
  nesting, `_FILE` suffix), passes it with a `TypeDescriptor` to `SchemaAssembler`, and passes the
  `Schema` to `ContractAssembler` with the caller's `App`. `schema.loader` is always empty: Spring
  decides the layers from its own property sources, so there are no loader variables to publish.
- **`SpringContractMain`** emits the contract at build time with no application context. Given the
  properties class's binary name, the prefix and `app.name`, plus `--nesting-separator` and
  `--file-suffix` for a dialect other than Spring's, it reads defaults off a default-constructed
  instance (what the binder leaves a field at when nothing supplies it) and writes the contract to
  `--out` or standard output. It needs `terrace-config-core-jackson2` on its classpath (see
  [JACKSON.md](JACKSON.md)) and says so when it is missing. There is no boot-time producer: a
  contract is a build artefact that describes the image, is conformed before the image exists and
  ships inside it, and one assembled by a running application would be a second, unconformed copy.
