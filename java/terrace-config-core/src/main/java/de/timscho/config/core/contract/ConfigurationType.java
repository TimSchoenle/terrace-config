package de.timscho.config.core.contract;

import de.timscho.config.core.descriptor.TypeDescriptor;
import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Modifier;

/**
 * A service's root configuration type, named on a build-time producer's command line rather than
 * referenced at compile time: its generated {@link TypeDescriptor}, and the instance whose field
 * initialisers are its defaults.
 *
 * <p>The descriptor {@code terrace-config-processor} generates is always top-level, named after
 * every enclosing simple name joined with {@code $} plus {@code Descriptor}, in the annotated
 * type's own package. That is exactly the type's binary name with {@code Descriptor} appended, so
 * a nested type needs no special case.
 *
 * @param type the {@code @TerraceConfig}-annotated class
 */
public record ConfigurationType(Class<?> type) {

    /** The field every generated descriptor class carries. */
    private static final String DESCRIPTOR_FIELD = "DESCRIPTOR";

    /**
     * The type of this binary name, loaded without initialising it — its static initialisers are
     * the service's, and the descriptor alone says everything a contract needs.
     *
     * @param binaryName e.g. {@code com.example.Config}, or {@code com.example.Outer$Inner}
     * @param loader     where to look
     * @throws IllegalArgumentException when no such class is on the classpath
     */
    public static ConfigurationType named(final String binaryName, final ClassLoader loader) {
        try {
            return new ConfigurationType(Class.forName(binaryName, false, loader));
        } catch (ClassNotFoundException e) {
            throw new IllegalArgumentException(binaryName + " is not on the classpath", e);
        }
    }

    /**
     * The descriptor generated for this type.
     *
     * @throws IllegalArgumentException when no generated descriptor is found beside the type: it is
     *                                  not annotated, or the processor did not run
     */
    public TypeDescriptor descriptor() {
        final String descriptorName = this.type.getName() + "Descriptor";
        final Class<?> descriptorClass;
        try {
            descriptorClass = Class.forName(descriptorName, true, this.type.getClassLoader());
        } catch (ClassNotFoundException e) {
            throw new IllegalArgumentException(
                    descriptorName + " was not found beside " + this.type.getName()
                            + ". Is the type annotated with @TerraceConfig, and did terrace-config-processor"
                            + " run as an annotation processor?",
                    e);
        }
        final Field field;
        try {
            field = descriptorClass.getField(DESCRIPTOR_FIELD);
        } catch (NoSuchFieldException e) {
            throw new IllegalArgumentException(
                    descriptorName + " carries no public " + DESCRIPTOR_FIELD
                            + " field, so terrace-config-processor did not generate it",
                    e);
        }
        if (!Modifier.isStatic(field.getModifiers()) || !TypeDescriptor.class.equals(field.getType())) {
            throw new IllegalArgumentException(descriptorName + "." + DESCRIPTOR_FIELD
                    + " is not a static TypeDescriptor, so terrace-config-processor did not generate it");
        }
        try {
            return (TypeDescriptor) field.get(null);
        } catch (IllegalAccessException e) {
            throw new IllegalArgumentException(descriptorName + "." + DESCRIPTOR_FIELD + " is not accessible", e);
        }
    }

    /**
     * A default-constructed instance: what a boot with no configuration at all binds, and so the
     * value whose fields are the published defaults.
     *
     * @throws IllegalArgumentException when the type has no no-argument constructor, or that
     *                                  constructor throws
     */
    public Object defaultInstance() {
        try {
            final Constructor<?> constructor = this.type.getDeclaredConstructor();
            // The same access Jackson takes when it binds the type at runtime: a configuration
            // class need not be public to be loaded, so it need not be public to be described.
            constructor.setAccessible(true);
            return constructor.newInstance();
        } catch (NoSuchMethodException | InstantiationException | IllegalAccessException e) {
            throw new IllegalArgumentException(
                    this.type.getName() + " needs a no-argument constructor: its defaults are"
                            + " the values a default-constructed instance holds",
                    e);
        } catch (InvocationTargetException e) {
            throw new IllegalArgumentException(
                    this.type.getName() + "'s no-argument constructor threw " + e.getCause(), e);
        }
    }
}
