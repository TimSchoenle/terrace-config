# Jackson 2 and 3

One model serves both Jackson majors, and each major gets its own codec module. This page covers
why the two majors can share a classpath, how the model deserialises under either, and what
`terrace-config-spring-boot` depends on.

## Why both majors fit on one classpath

Jackson 3 moved its group ID and Java packages from `com.fasterxml.jackson` to `tools.jackson`,
except `jackson-annotations`, which stays on `com.fasterxml.jackson` 2.x so both majors can depend
on it. A 2.x and a 3.x artefact are therefore different coordinates rather than two versions of one
dependency, and they coexist on one classpath with nothing for Gradle or Maven to reconcile. That is
what allows one model usable under either major instead of two model copies.

## The model carries both sets of annotations

`@Jacksonized`, Lombok's add-on for `@Builder`, normally emits one `@JsonDeserialize(builder = ...)`
and `@JsonPOJOBuilder` pair. Both annotation types live in Jackson's `databind` package, which
differs between the majors, so by default a Lombok-built model deserialises only under the major it
was generated for. `lombok.jacksonized.jacksonVersion` makes `@Jacksonized` emit both pairs, and
`java/lombok.config` sets it to `2` and `3`.

`terrace-config-core` declares both databind artefacts `compileOnly`. The model needs the
annotation types at compile time and neither `ObjectMapper` at runtime, so depending on `-core`
alone pulls in no Jackson major.

## One codec module per major

`terrace-config-core-jackson2` and `terrace-config-core-jackson3` each provide
`de.timscho.config.core.io.ContractCodec`, the byte-stable `json` reader and writer for `-core`'s
`Contract`. They share the package, the class name and the public `read` and `write` methods, so a
consumer picks the module that matches the Jackson major it already has and the call sites are the
same either way. Neither depends on the other; each depends only on `-core` and its own Jackson
major.

`ContractCodec` builds its `ObjectMapper` once, statically. Its
`CompactEmptyContainerPrettyPrinter` exists because Jackson's `DefaultPrettyPrinter` renders an
empty array as `[ ]` and a field separator as `"key" : value`, where the corpus uses `[]` and
`"key": value`. `spec/v1/FORMAT.md`'s "Publication" section defines the document as byte-stable, so
the difference is not cosmetic: it is what makes `ContractCorpusRoundTripTest` possible.

The Jackson 3 copy differs from the Jackson 2 one only where the API does:

- `writeObjectFieldValueSeparator` is `writeObjectNameValueSeparator`;
- the `ObjectMapper` is built immutably through `JsonMapper.builder()` instead of being configured
  after construction;
- its streaming methods throw the unchecked `JacksonException` instead of `IOException`.

## `terrace-config-spring-boot`

`terrace-config-spring-boot` declares no Jackson dependency at runtime. It reads JSON only through
Spring's own `Binder` and `ObjectMapper`, so it follows whichever Jackson major the application's
Spring Boot BOM resolves.

`SpringContractMain` runs at build time and writes the contract with `-core-jackson2`. It compiles
against that module `compileOnly`, checks for it before first use, and finds it on the Gradle
plugin's `terraceContractGenerator` classpath, never on the service's.
