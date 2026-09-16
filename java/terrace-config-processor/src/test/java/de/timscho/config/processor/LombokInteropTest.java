package de.timscho.config.processor;

import static com.google.testing.compile.CompilationSubject.assertThat;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.IOException;
import java.lang.reflect.Constructor;
import java.util.Optional;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

import javax.annotation.processing.Processor;
import javax.tools.JavaFileObject;

import com.google.testing.compile.Compilation;
import com.google.testing.compile.Compiler;
import com.google.testing.compile.JavaFileObjects;
import org.junit.jupiter.api.Test;

/**
 * Proves {@link FieldResolver#hasLombokBuilderDefault} actually works, by running this
 * processor's own {@link TerraceConfigProcessor} alongside Lombok's real processor in the same
 * {@code compile-testing} compilation — the only way to reproduce the exact failure this class
 * exists to fix. {@link #lombokProcessor} loads Lombok's actual SPI-registered class,
 * {@code lombok.launch.AnnotationProcessorHider$AnnotationProcessor}, reflectively: it is
 * package-private, so nothing outside {@code lombok.launch} can import or {@code new} it, but a
 * {@link Processor} reference to an instance of it is exactly as usable as one to any other
 * processor — {@code compile-testing}'s own {@code withProcessors} takes the interface, never the
 * concrete type.
 *
 * <p><b>The bug this guards against.</b> A field like {@code @Builder.Default String bindAddr =
 * "127.0.0.1";} has a real, working default — Lombok's generated builder falls back to it exactly
 * when a bean-style field initializer would. But Lombok's {@code @Builder} handler rewrites the
 * field's AST node in place, during its own annotation-processing round: the initializer is
 * lifted into a synthetic {@code $default$bindAddr()} method and the field declaration's own
 * initializer is nulled out. Whichever processor's round runs first is not something either
 * guarantees — {@code withProcessors} above deliberately lists Lombok's processor before this
 * one, forcing its rewrite to happen first, because that ordering is what actually exercises the
 * bug: with {@link FieldResolver#hasLombokBuilderDefault} temporarily disabled (only {@link
 * FieldResolver#hasInitializer}, reading the compiler's public {@code Trees} API), this test
 * fails in that order and passes in the reverse one — proof the field's initializer really can be
 * gone by the time this processor's own round runs, not just in theory. {@code
 * hasLombokBuilderDefault} reads {@code @Builder.Default}'s own presence instead, a signal
 * Lombok's rewrite does not touch.
 */
class LombokInteropTest {

    @Test
    void builderDefaultFieldIsSeenAsHavingADefault() throws IOException, ReflectiveOperationException {
        Compilation compilation = Compiler.javac()
                .withProcessors(lombokProcessor(), new TerraceConfigProcessor())
                .compile(JavaFileObjects.forSourceLines(
                        "test.LombokConfig",
                        "package test;",
                        "import de.timscho.config.annotations.TerraceConfig;",
                        "import lombok.Builder;",
                        "import lombok.Value;",
                        "@TerraceConfig",
                        "@Value",
                        "@Builder",
                        "public class LombokConfig {",
                        "    /** Address the HTTP listener binds to. */",
                        "    @Builder.Default",
                        "    String bindAddr = \"127.0.0.1\";",
                        "    /** Port the HTTP listener binds to. */",
                        "    int port;",
                        "}"));

        assertThat(compilation).succeededWithoutWarnings();

        Optional<JavaFileObject> generated = compilation.generatedSourceFile("test.LombokConfigDescriptor");
        assertTrue(generated.isPresent(), "expected a generated test.LombokConfigDescriptor");
        String content = generated.get().getCharContent(false).toString();

        assertTrue(
                lastBooleanArgumentFor(content, "bindAddr"),
                "a `@Builder.Default` field must resolve to `hasDefault = true`:\n" + content);
        assertTrue(
                !lastBooleanArgumentFor(content, "port"),
                "a field with no default at all must resolve to `hasDefault = false`:\n" + content);
    }

    /** A fresh instance of Lombok's own processor, loaded by name — see the class doc for why. */
    private static Processor lombokProcessor() throws ReflectiveOperationException {
        Class<?> hider = Class.forName("lombok.launch.AnnotationProcessorHider$AnnotationProcessor");
        Constructor<?> constructor = hider.getDeclaredConstructor();
        constructor.setAccessible(true);
        return (Processor) constructor.newInstance();
    }

    /**
     * {@code KeyDescriptor}'s {@code hasDefault} argument for the field named {@code fieldName}'s
     * own generated call. {@code hasDefault} is the second-to-last constructor argument, followed
     * only by the trailing {@code aliases} list — anchoring on that fixed {@code
     * , java.util.List.of(...))} suffix finds it regardless of the (possibly non-empty) alias
     * list's own contents.
     */
    private static boolean lastBooleanArgumentFor(String content, String fieldName) {
        Pattern pattern = Pattern.compile(
                "KeyDescriptor\\(\"" + fieldName + "\".*?, (true|false), java\\.util\\.List\\.of\\([^()]*\\)\\)");
        Matcher matcher = pattern.matcher(content);
        assertTrue(matcher.find(), "no generated KeyDescriptor call found for field '" + fieldName + "':\n" + content);
        return Boolean.parseBoolean(matcher.group(1));
    }
}
