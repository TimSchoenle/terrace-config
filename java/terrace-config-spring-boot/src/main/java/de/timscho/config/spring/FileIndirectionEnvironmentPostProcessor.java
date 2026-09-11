package de.timscho.config.spring;

import java.io.IOException;
import java.nio.charset.CharacterCodingException;
import java.nio.charset.CodingErrorAction;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Optional;

import org.springframework.boot.SpringApplication;
import org.springframework.boot.context.config.ConfigDataEnvironmentPostProcessor;
import org.springframework.boot.env.EnvironmentPostProcessor;
import org.springframework.core.Ordered;
import org.springframework.core.env.ConfigurableEnvironment;
import org.springframework.core.env.EnumerablePropertySource;
import org.springframework.core.env.MapPropertySource;
import org.springframework.core.env.MutablePropertySources;
import org.springframework.core.env.PropertySource;
import org.springframework.core.env.StandardEnvironment;

import org.jetbrains.annotations.Blocking;

/**
 * The one layer Spring's own binder does not already give it: {@code <NAME>_FILE=/path} naming a
 * file whose contents supply the key {@code <NAME>} would otherwise have named directly —
 * exactly the convention a mounted Kubernetes {@code Secret} or a Docker Swarm secret is
 * addressed by, and exactly what {@code terrace-config-loader}'s own {@code FileSuffixEnv} layer
 * does for the vanilla loader.
 *
 * <p>Runs once, early — {@link EnvironmentPostProcessor}s run before any bean, including the
 * application's own, is created — over whatever the system environment's own property source
 * ({@link StandardEnvironment#SYSTEM_ENVIRONMENT_PROPERTY_SOURCE_NAME}) enumerates. Every {@code
 * _FILE} variable found becomes one entry in a new {@link MapPropertySource}, keyed by {@link
 * SpringDialect#propertyName(String)} and added at the front of the environment's property
 * sources — the same "file layers win" precedence {@code terrace-config-loader}'s own {@code
 * FileLayers} documents, since a mounted secret is assumed more specific than whatever a plain
 * environment variable or configuration file already said.
 *
 * <p>Registered the way every {@link EnvironmentPostProcessor} still is in Spring Boot 3, via
 * {@code META-INF/spring.factories} — this class runs before the {@code
 * AutoConfiguration.imports} mechanism a starter's own auto-configuration would use even exists.
 */
public final class FileIndirectionEnvironmentPostProcessor implements EnvironmentPostProcessor, Ordered {

    /**
     * One position after {@link ConfigDataEnvironmentPostProcessor}, so {@code application.yml},
     * profile-specific files and every other config-data source are already loaded — and so
     * {@code addFirst} below outranks all of them, not just the ones registered earlier.
     */
    public static final int ORDER = ConfigDataEnvironmentPostProcessor.ORDER + 1;

    private static final String PROPERTY_SOURCE_NAME = "terraceFileIndirection";

    @Override
    public int getOrder() {
        return ORDER;
    }

    @Override
    public void postProcessEnvironment(ConfigurableEnvironment environment, SpringApplication application) {
        MutablePropertySources sources = environment.getPropertySources();
        PropertySource<?> systemEnvironment =
                sources.get(StandardEnvironment.SYSTEM_ENVIRONMENT_PROPERTY_SOURCE_NAME);
        if (!(systemEnvironment instanceof EnumerablePropertySource<?> enumerable)) {
            return;
        }

        SpringDialect dialect = SpringDialect.standard();
        Map<String, Object> resolved = new LinkedHashMap<>();
        for (String name : enumerable.getPropertyNames()) {
            Optional<String> target = dialect.indirectionTarget(name);
            if (target.isEmpty()) {
                continue;
            }
            Object value = enumerable.getProperty(name);
            if (!(value instanceof String pathText) || pathText.isBlank()) {
                continue;
            }
            String fileValue = readValue(name, Path.of(pathText));
            resolved.put(dialect.propertyName(target.get()), fileValue);
        }

        if (!resolved.isEmpty()) {
            sources.addFirst(new MapPropertySource(PROPERTY_SOURCE_NAME, resolved));
        }
    }

    /**
     * The contents of one value file, minus trailing line terminators — only {@code \r}/{@code
     * \n}, never spaces or tabs, since a trailing space can be a real character of a real
     * credential.
     */
    @Blocking
    private static String readValue(String envName, Path path) {
        byte[] bytes;
        try {
            bytes = Files.readAllBytes(path);
        } catch (IOException e) {
            throw new FileIndirectionException(
                    envName + " names " + path + ", which could not be read: " + e.getMessage(), e);
        }
        String text = decodeStrictUtf8(envName, path, bytes);
        int end = text.length();
        while (end > 0 && (text.charAt(end - 1) == '\r' || text.charAt(end - 1) == '\n')) {
            end--;
        }
        return text.substring(0, end);
    }

    private static String decodeStrictUtf8(String envName, Path path, byte[] bytes) {
        try {
            return StandardCharsets.UTF_8
                    .newDecoder()
                    .onMalformedInput(CodingErrorAction.REPORT)
                    .onUnmappableCharacter(CodingErrorAction.REPORT)
                    .decode(java.nio.ByteBuffer.wrap(bytes))
                    .toString();
        } catch (CharacterCodingException e) {
            throw new FileIndirectionException(envName + " names " + path + ", which is not valid UTF-8", e);
        }
    }
}
