package de.timscho.config.processor;

import org.junit.jupiter.api.Test;

import com.google.testing.compile.Compilation;
import com.google.testing.compile.Compiler;
import com.google.testing.compile.JavaFileObjects;

import static com.google.testing.compile.CompilationSubject.assertThat;

/**
 * One test per resolvable annotation and one per refusal, mirroring {@code SCHEMA.md}'s own
 * derive test shape: "one test per resolvable annotation, and one per refusal asserting the
 * diagnostic text".
 */
class TerraceConfigProcessorTest {

    private static Compilation compile(String className, String... sourceLines) {
        return Compiler.javac()
                .withProcessors(new TerraceConfigProcessor())
                .compile(JavaFileObjects.forSourceLines(className, sourceLines));
    }

    // ---- Resolvable shapes ----------------------------------------------------------------

    @Test
    void plainLeafFieldsNeedNoAnnotation() {
        Compilation compilation = compile("test.Config",
                "package test;",
                "import de.timscho.config.annotations.TerraceConfig;",
                "@TerraceConfig",
                "public class Config {",
                "    /** A username. */",
                "    String username;",
                "    int port;",
                "    boolean enabled;",
                "}");
        assertThat(compilation).succeededWithoutWarnings();
        assertThat(compilation).generatedSourceFile("test.ConfigDescriptor");
    }

    @Test
    void nestedField() {
        Compilation compilation = compile("test.Outer",
                "package test;",
                "import de.timscho.config.annotations.TerraceConfig;",
                "import de.timscho.config.annotations.Nested;",
                "public class Outer {",
                "    @TerraceConfig",
                "    public static class Inner {",
                "        String value;",
                "    }",
                "    @TerraceConfig",
                "    public static class Root {",
                "        /** The nested block. */",
                "        @Nested",
                "        Inner inner;",
                "    }",
                "}");
        assertThat(compilation).succeededWithoutWarnings();
        assertThat(compilation).generatedSourceFile("test.Outer$RootDescriptor");
    }

    @Test
    void bareValuesOnATerraceConfigEnum() {
        Compilation compilation = compile("test.LevelConfig",
                "package test;",
                "import de.timscho.config.annotations.TerraceConfig;",
                "import de.timscho.config.annotations.Values;",
                "public class LevelConfig {",
                "    @TerraceConfig",
                "    public enum Level { TRACE, DEBUG, INFO }",
                "    @TerraceConfig",
                "    public static class Root {",
                "        @Values",
                "        Level level;",
                "    }",
                "}");
        assertThat(compilation).succeededWithoutWarnings();
    }

    @Test
    void valuesFromAMirror() {
        Compilation compilation = compile("test.MirrorConfig",
                "package test;",
                "import de.timscho.config.annotations.TerraceConfig;",
                "import de.timscho.config.annotations.Values;",
                "public class MirrorConfig {",
                "    public enum Foreign { GZIP, ZSTD }",
                "    @TerraceConfig",
                "    public enum ForeignMirror { GZIP, ZSTD }",
                "    @TerraceConfig",
                "    public static class Root {",
                "        @Values(from = ForeignMirror.class)",
                "        Foreign compression;",
                "    }",
                "}");
        assertThat(compilation).succeededWithoutWarnings();
    }

    @Test
    void literalValuesList() {
        Compilation compilation = compile("test.LiteralConfig",
                "package test;",
                "import de.timscho.config.annotations.TerraceConfig;",
                "import de.timscho.config.annotations.Values;",
                "@TerraceConfig",
                "public class LiteralConfig {",
                "    @Values({\"gzip\", \"zstd\", \"none\"})",
                "    Object compression;",
                "}");
        assertThat(compilation).succeededWithoutWarnings();
    }

    @Test
    void rangeOnANumericField() {
        Compilation compilation = compile("test.RangeConfig",
                "package test;",
                "import de.timscho.config.annotations.TerraceConfig;",
                "import de.timscho.config.annotations.Range;",
                "@TerraceConfig",
                "public class RangeConfig {",
                "    @Range(min = 0.0, max = 1.0)",
                "    float sampleRate;",
                "}");
        assertThat(compilation).succeededWithoutWarnings();
    }

    @Test
    void elementOnAContainerOfAStruct() {
        Compilation compilation = compile("test.RoutesConfig",
                "package test;",
                "import java.util.List;",
                "import de.timscho.config.annotations.TerraceConfig;",
                "import de.timscho.config.annotations.Element;",
                "public class RoutesConfig {",
                "    @TerraceConfig",
                "    public static class Route {",
                "        String upstream;",
                "    }",
                "    @TerraceConfig",
                "    public static class Root {",
                "        @Element",
                "        List<Route> routes;",
                "    }",
                "}");
        assertThat(compilation).succeededWithoutWarnings();
    }

    @Test
    void elementValuesOnAContainerOfAnEnum() {
        Compilation compilation = compile("test.MethodsConfig",
                "package test;",
                "import java.util.List;",
                "import de.timscho.config.annotations.TerraceConfig;",
                "import de.timscho.config.annotations.ElementValues;",
                "public class MethodsConfig {",
                "    @TerraceConfig",
                "    public enum Method { GET, POST }",
                "    @TerraceConfig",
                "    public static class Root {",
                "        @ElementValues",
                "        List<Method> methods;",
                "    }",
                "}");
        assertThat(compilation).succeededWithoutWarnings();
    }

    @Test
    void skipOmitsTheField() {
        Compilation compilation = compile("test.SkipConfig",
                "package test;",
                "import de.timscho.config.annotations.TerraceConfig;",
                "import de.timscho.config.annotations.Skip;",
                "@TerraceConfig",
                "public class SkipConfig {",
                "    @Skip",
                "    Object anything;",
                "}");
        assertThat(compilation).succeededWithoutWarnings();
    }

    // ---- Refusals ---------------------------------------------------------------------------

    @Test
    void unresolvedFieldTypeIsACompileError() {
        Compilation compilation = compile("test.Unresolved",
                "package test;",
                "import de.timscho.config.annotations.TerraceConfig;",
                "@TerraceConfig",
                "public class Unresolved {",
                "    Object anything;",
                "}");
        assertThat(compilation).failed();
        assertThat(compilation).hadErrorContaining("anything");
        assertThat(compilation).hadErrorContaining("publishes no shape");
    }

    @Test
    void rangeOnANonNumericFieldIsACompileError() {
        Compilation compilation = compile("test.BadRange",
                "package test;",
                "import de.timscho.config.annotations.TerraceConfig;",
                "import de.timscho.config.annotations.Range;",
                "@TerraceConfig",
                "public class BadRange {",
                "    @Range(min = 0.0)",
                "    String notANumber;",
                "}");
        assertThat(compilation).failed();
        assertThat(compilation).hadErrorContaining("needs a numeric type");
    }

    @Test
    void rangeWithNoBoundSetIsACompileError() {
        Compilation compilation = compile("test.EmptyRange",
                "package test;",
                "import de.timscho.config.annotations.TerraceConfig;",
                "import de.timscho.config.annotations.Range;",
                "@TerraceConfig",
                "public class EmptyRange {",
                "    @Range",
                "    int port;",
                "}");
        assertThat(compilation).failed();
        assertThat(compilation).hadErrorContaining("has none of min/max/exclusiveMin/exclusiveMax set");
    }

    @Test
    void nestedOnANonAnnotatedTypeIsACompileError() {
        Compilation compilation = compile("test.BadNested",
                "package test;",
                "import de.timscho.config.annotations.TerraceConfig;",
                "import de.timscho.config.annotations.Nested;",
                "public class BadNested {",
                "    public static class Plain { String value; }",
                "    @TerraceConfig",
                "    public static class Root {",
                "        @Nested",
                "        Plain plain;",
                "    }",
                "}");
        assertThat(compilation).failed();
        assertThat(compilation).hadErrorContaining("needs a type annotated @TerraceConfig as a struct");
    }

    @Test
    void containerElementWithNoShapeIsACompileError() {
        Compilation compilation = compile("test.BadContainer",
                "package test;",
                "import java.util.List;",
                "import de.timscho.config.annotations.TerraceConfig;",
                "@TerraceConfig",
                "public class BadContainer {",
                "    List<Object> items;",
                "}");
        assertThat(compilation).failed();
        assertThat(compilation).hadErrorContaining("publishes no shape");
    }

    @Test
    void conflictingShapeAnnotationsIsACompileError() {
        Compilation compilation = compile("test.Conflicting",
                "package test;",
                "import de.timscho.config.annotations.TerraceConfig;",
                "import de.timscho.config.annotations.Nested;",
                "import de.timscho.config.annotations.Range;",
                "@TerraceConfig",
                "public class Conflicting {",
                "    @TerraceConfig",
                "    public static class Inner { String value; }",
                "    @Nested",
                "    @Range(min = 0)",
                "    Inner inner;",
                "}");
        assertThat(compilation).failed();
        assertThat(compilation).hadErrorContaining("carries more than one of @Nested/@Values/@Range");
    }
}
