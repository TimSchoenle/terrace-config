# Null-safety

Every library package is `@NullMarked`, so a reference is non-null unless it is annotated
`@Nullable`. This page covers which annotation library does what, how each is declared, and how
the annotations reach Lombok's generated code.

## The default

Every library package under `src/main/java` carries a `package-info.java` with jspecify's
`@NullMarked` (`org.jspecify.annotations`): every field, parameter and return type in the package is
non-null unless annotated `@Nullable`. The example modules' packages carry documentation only.

## Why jspecify, and what JetBrains annotations are for

Both jspecify and `org.jetbrains.annotations` define nullness annotations, and using both for the
same purpose would give one audience two vocabularies. jspecify is the vendor-neutral standard
([jspecify.dev](https://jspecify.dev)) that IntelliJ, the Checker Framework, NullAway and Error
Prone read, so nullness is jspecify's alone. javac records the annotations but enforces nothing, and
this build runs no nullness checker, so a violation surfaces in an IDE or an external checker rather
than as a compile error.

`org.jetbrains.annotations` is declared for the inspection vocabulary jspecify does not cover:

- `@Contract(pure = true)` on `ContractAssembler.deriveConstraint`, written by its qualified name
  because the codebase's own `Contract` model type would collide with the simple name;
- `@Blocking` on every method that performs file or process I/O, such as both copies of
  `ContractCodec.write(Contract, Path)` and `read(Path)`, `MetaSchemaValidator.load()` and
  `FileIndirectionEnvironmentPostProcessor`'s file read.

## How each is declared

- **jspecify** is `implementation`, as [its own guidance](https://jspecify.dev/docs/using/)
  recommends, because its annotations have `RUNTIME` retention and the jar is small. The one
  exception is `terrace-config-annotations`, which declares it `compileOnly`: the module promises a
  consumer nothing beyond the JDK, and every type in it is a `SOURCE`-retention `@interface`, where
  `@Nullable` cannot apply to an annotation element.
- **`org.jetbrains.annotations`** is `compileOnly` everywhere. Every annotation in it has `CLASS`
  retention, so it is never needed at runtime, not even reflectively. `terrace-config.java-conventions`
  declares it once for every module.

## Lombok

jspecify's `@Nullable` is `TYPE_USE`-only and is written on the field's type
(`@Nullable String foo;`). On its own it stops at the field and never reaches the getter return
type, builder setter parameter or constructor parameter Lombok generates, which are the API those
classes expose. `java/lombok.config` lists `org.jspecify.annotations.Nullable` under
`lombok.copyableAnnotations`, so Lombok copies it onto every generated signature it belongs on.

## What is `@Nullable`

Only a value that can be absent:

- a model field the meta-schema marks optional, such as `Key.env`, `ExternalVar.owner` or
  `App.version`;
- a private helper's `null` sentinel return, such as `SchemaAssembler`'s constraint builders,
  `Origin.fromSources` or `TomlLayers.fragmentKeys`;
- the parameter of an `Object.equals(Object)` override, as in `JsonSchemaDocument`;
- `ProducerArguments.optional`'s absent option.

Two exceptions are deliberate:

- The four fields `TerraceConfigProcessor` assigns in its overridden `init`, rather than in a
  constructor, are non-null by convention. That is the initializer-method exemption NullAway grants
  framework lifecycle methods; the package's `package-info.java` documents it.
- `lombok.NonNull` is not used. It is a runtime guard, which is a different concern from
  jspecify's static contract, and a required field is non-null by its package's `@NullMarked`
  default alone.

`ContainerShape.element()` is `@Nullable` because the `NONE` sentinel holds `null`.
`FieldResolver.resolveContainerField` reads it only when `kind() != NONE`, and throws
`IllegalStateException` if that invariant is ever broken instead of relying on it silently.
