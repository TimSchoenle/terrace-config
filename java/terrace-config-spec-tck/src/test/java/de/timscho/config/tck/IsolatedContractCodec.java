package de.timscho.config.tck;

import java.io.File;
import java.io.IOException;
import java.io.UncheckedIOException;
import java.lang.reflect.Method;
import java.net.MalformedURLException;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.file.Paths;
import java.util.Arrays;

/**
 * Round-trips a document through one Jackson backend's own {@code de.timscho.config.core.io
 * .ContractCodec}, loaded into an isolated {@link URLClassLoader} built from that backend's own
 * resolved runtime classpath.
 *
 * <p>{@code terrace-config-core-jackson2} and {@code terrace-config-core-jackson3} deliberately
 * publish a class of that exact same name — so that a consumer never has to know which one it
 * linked against — which also means the two can never sit on one classloader's classpath at once
 * without one shadowing the other. Isolating each into its own loader, parented at the platform
 * loader rather than this test's own context loader, is what lets {@link JacksonParityTest} ask
 * both the same question in one JVM without either seeing the other, or seeing this module's own
 * classes at all.
 */
final class IsolatedContractCodec implements AutoCloseable {

    private final URLClassLoader classLoader;
    private final Class<?> contractCodecClass;
    private final Class<?> contractClass;

    private IsolatedContractCodec(
            final URLClassLoader classLoader, final Class<?> contractCodecClass, final Class<?> contractClass) {
        this.classLoader = classLoader;
        this.contractCodecClass = contractCodecClass;
        this.contractClass = contractClass;
    }

    /**
     * {@code classpath} as {@link File#pathSeparator}-joined entries, e.g. a resolved Gradle
     * configuration's {@code asPath}.
     */
    static IsolatedContractCodec forClasspath(final String classpath) {
        final URL[] urls = Arrays.stream(classpath.split(File.pathSeparator))
                .filter(entry -> !entry.isBlank())
                .map(IsolatedContractCodec::toUrl)
                .toArray(URL[]::new);
        final URLClassLoader loader = new URLClassLoader(urls, ClassLoader.getPlatformClassLoader());
        try {
            final Class<?> codec = Class.forName("de.timscho.config.core.io.ContractCodec", true, loader);
            final Class<?> contract = Class.forName("de.timscho.config.core.model.Contract", true, loader);
            return new IsolatedContractCodec(loader, codec, contract);
        } catch (ClassNotFoundException e) {
            throw new IllegalStateException(
                    "de.timscho.config.core.io.ContractCodec not found on isolated classpath: " + classpath, e);
        }
    }

    /**
     * {@code ContractCodec.write(ContractCodec.read(json))}, entirely inside the isolated loader:
     * the {@code Contract} instance {@code read} produces never leaves it, so this never risks a
     * {@link ClassCastException} against another loader's copy of the same class.
     */
    byte[] roundTrip(final byte[] json) {
        try {
            final Method read = contractCodecClass.getMethod("read", byte[].class);
            final Object contract = read.invoke(null, (Object) json);
            final Method write = contractCodecClass.getMethod("write", contractClass);
            return (byte[]) write.invoke(null, contract);
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("isolated ContractCodec round trip failed", e);
        }
    }

    private static URL toUrl(final String path) {
        try {
            return Paths.get(path).toUri().toURL();
        } catch (MalformedURLException e) {
            throw new IllegalStateException("not a usable classpath entry: " + path, e);
        }
    }

    @Override
    public void close() {
        try {
            classLoader.close();
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
    }
}
