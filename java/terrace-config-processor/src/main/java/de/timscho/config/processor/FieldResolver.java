package de.timscho.config.processor;

import java.util.List;

import javax.annotation.processing.ProcessingEnvironment;
import javax.lang.model.element.AnnotationMirror;
import javax.lang.model.element.Element;
import javax.lang.model.element.TypeElement;
import javax.lang.model.element.VariableElement;
import javax.lang.model.type.DeclaredType;
import javax.lang.model.type.MirroredTypeException;
import javax.lang.model.type.TypeMirror;
import javax.lang.model.util.Elements;

import com.sun.source.tree.Tree;
import com.sun.source.tree.VariableTree;
import com.sun.source.util.Trees;
import org.jspecify.annotations.Nullable;

import de.timscho.config.annotations.ElementValues;
import de.timscho.config.annotations.Nested;
import de.timscho.config.annotations.Note;
import de.timscho.config.annotations.Range;
import de.timscho.config.annotations.Secret;
import de.timscho.config.annotations.TerraceConfig;
import de.timscho.config.annotations.Values;
import de.timscho.config.core.descriptor.KeyDescriptor.ContainerKind;

/**
 * Resolves one field of a {@code @TerraceConfig} struct to the Java source of a {@code new
 * KeyDescriptor(...)} expression, or throws {@link DescriptorException} naming the field, its
 * type and the attributes that would resolve it — the diagnostic {@code rust/docs/SCHEMA.md}
 * calls "the whole point of the feature".
 */
final class FieldResolver {

    private final Elements elements;
    private final Trees trees;

    FieldResolver(final ProcessingEnvironment env) {
        this.elements = env.getElementUtils();
        // `Trees` is the compiler's own public tree API (`com.sun.source.util`/`com.sun.source.tree`,
        // exported unconditionally by `jdk.compiler` since Java 9 — unlike `com.sun.tools.javac.*`,
        // it needs no `--add-exports`), the only way this processor can see whether a field
        // declaration carries an initializer at all: `VariableElement` alone answers that only for
        // a `static final` constant (`getConstantValue()`), never for an ordinary instance field
        // like `private String port = "8080";`.
        this.trees = Trees.instance(env);
    }

    /** {@code null} means the field carries {@code @Skip} and contributes no key. */
    @Nullable String resolve(final VariableElement field) {
        if (field.getAnnotation(de.timscho.config.annotations.Skip.class) != null) {
            return null;
        }

        // `@JsonProperty`'s value when present, exactly as `TerraceConfigProcessor.renderEnum`
        // already reads it for an enum constant — a field renamed for Jackson's own binding is a
        // field the loader actually reads under that name, and a key path built from the Java
        // identifier instead would describe an environment variable nothing in the loader
        // responds to.
        final String name =
                JacksonReflection.jsonPropertyName(field, field.getSimpleName().toString());
        final List<String> aliases = JacksonReflection.jsonAliases(field);
        final String docs = Javadocs.normalize(elements.getDocComment(field));
        final String summary = Javadocs.summary(docs);
        final boolean secret = field.getAnnotation(Secret.class) != null;
        final Note noteAnn = field.getAnnotation(Note.class);
        final String note = noteAnn == null ? null : noteAnn.value();

        final TypeMirror declaredType = field.asType();
        final String typeName = TypeNames.simplify(declaredType.toString());
        final ContainerShape shape = ContainerShape.of(declaredType);

        final Shape resolved = shape.kind() == ContainerKind.NONE
                ? resolveLeafField(field, declaredType)
                : resolveContainerField(field, shape);

        return "new de.timscho.config.core.descriptor.KeyDescriptor("
                + CodeGen.stringLiteral(name) + ", "
                + CodeGen.stringLiteral(docs) + ", "
                + CodeGen.stringLiteral(summary) + ", "
                + CodeGen.stringLiteral(typeName) + ", "
                + "de.timscho.config.core.descriptor.KeyDescriptor.ContainerKind." + shape.kind() + ", "
                + secret + ", "
                + CodeGen.nullableStringLiteral(note) + ", "
                + resolved.values + ", "
                + resolved.range + ", "
                + resolved.nestedKeys + ", "
                + resolved.element + ", "
                + resolved.closed + ", "
                + hasDefault(field) + ", "
                + CodeGen.stringListLiteral(aliases)
                + ")";
    }

    /**
     * Whether {@code field} carries a default — the Java equivalent of a Rust field carrying
     * {@code #[serde(default = "…")]}, and what {@link
     * de.timscho.config.core.descriptor.SchemaAssembler} uses to decide {@link
     * de.timscho.config.core.model.Key#isRequired()}. Two independent signals, because one field
     * shape hides the other: {@link #hasInitializer} sees a plain {@code private String port =
     * "8080";}, but not a Lombok {@code @Builder.Default} field, whose initializer {@link
     * #hasLombokBuilderDefault} has to find a different way — see that method for why.
     */
    private boolean hasDefault(final VariableElement field) {
        return hasInitializer(field) || hasLombokBuilderDefault(field);
    }

    /**
     * Whether {@code field}'s own declaration carries an initializer, read straight off the
     * compiler's own tree for it. {@code VariableElement} alone answers this only for a {@code
     * static final} constant ({@code getConstantValue()}), never for an ordinary instance field —
     * this is the only way to see one on a field Lombok has not touched. A field the compiler
     * cannot recover a tree for (only possible for one resolved from a previously compiled {@code
     * .class}, never a field this processor is generating a descriptor for in the same round) is
     * treated as having none, the conservative reading — the same one an absent initializer
     * itself gets.
     */
    private boolean hasInitializer(final VariableElement field) {
        final Tree tree = trees.getTree(field);
        return tree instanceof VariableTree variableTree && variableTree.getInitializer() != null;
    }

    /**
     * Whether {@code field} carries {@code @lombok.Builder.Default}, read by qualified name off
     * its {@link AnnotationMirror}s rather than importing the annotation type — the same reason
     * {@link JacksonReflection} reads Jackson's own annotations that way: this processor takes no
     * compile dependency on Lombok, and an annotated service may not have it on its own classpath
     * at all.
     *
     * <p><b>Why {@link #hasInitializer} cannot see this field's default.</b> Lombok's {@code
     * @Builder}/{@code @Jacksonized} handler rewrites a {@code @Builder.Default} field's AST node
     * in place, during its own annotation-processing round: the initializer is lifted out into a
     * synthetic {@code private static X $default$fieldName()} method, and the field declaration's
     * own initializer is nulled out — verified empirically against this exact processor (compiled
     * a small {@code @Value @Builder @Jacksonized} type with a {@code @Builder.Default} field and
     * inspected the generated descriptor: {@code hasInitializer} alone reports {@code false} for
     * it). Whether Lombok's round runs before this processor's own is not something either
     * processor's ordering guarantees, so by the time {@link #hasInitializer} calls {@link
     * Trees#getTree}, the initializer may already be gone — the same {@code JCTree} object,
     * mutated, not a separate pre-Lombok snapshot. The annotation's own presence is the
     * unaffected signal: Lombok refuses to compile {@code @Builder.Default} on a field with no
     * initializer at all, so seeing the annotation is itself proof one was written, whatever the
     * tree looks like by the time this processor reads it.
     */
    private boolean hasLombokBuilderDefault(final VariableElement field) {
        for (AnnotationMirror mirror : field.getAnnotationMirrors()) {
            if (mirror.getAnnotationType().toString().equals("lombok.Builder.Default")) {
                return true;
            }
        }
        return false;
    }

    private static final class Shape {
        String values = "java.util.List.of()";
        String range = "null";
        String nestedKeys = "java.util.List.of()";
        String element = "null";
        boolean closed;
    }

    private Shape resolveLeafField(final VariableElement field, final TypeMirror declaredType) {
        final Shape shape = new Shape();
        final Nested nested = field.getAnnotation(Nested.class);
        final Values values = field.getAnnotation(Values.class);
        final Range range = field.getAnnotation(Range.class);

        final int annotationCount = (nested != null ? 1 : 0) + (values != null ? 1 : 0) + (range != null ? 1 : 0);
        if (annotationCount > 1) {
            throw error(field, "carries more than one of @Nested/@Values/@Range; exactly one describes a field");
        }

        if (nested != null) {
            final TypeElement nestedType = requireTerraceConfigStruct(field, declaredType, "@Nested");
            shape.nestedKeys = descriptorReference(nestedType) + ".DESCRIPTOR.keys()";
            shape.closed = JacksonReflection.isClosed(nestedType);
            return shape;
        }
        if (values != null) {
            shape.values = resolveValues(field, declaredType, values::from, values.value(), "@Values");
            return shape;
        }
        if (range != null) {
            requireNumericLeaf(field, declaredType, "@Range");
            shape.range = rangeExpression(field, range);
            return shape;
        }
        if (LeafTypes.isLeaf(declaredType)) {
            return shape;
        }
        throw mustSayNothingError(field, declaredType);
    }

    private Shape resolveContainerField(final VariableElement field, final ContainerShape shape) {
        final Shape result = new Shape();
        final TypeMirror elementType = shape.element();
        if (elementType == null) {
            // Unreachable in practice: `ContainerShape.of` never pairs a non-`NONE` kind (the
            // only way to reach this method — see `resolve`) with a `null` element.
            throw new IllegalStateException("container shape " + shape.kind() + " has no element type");
        }

        if (field.getAnnotation(Nested.class) != null || field.getAnnotation(Values.class) != null) {
            throw error(field, "is a container field; use @Element/@ElementValues, not @Nested/@Values");
        }

        final de.timscho.config.annotations.Element elementAnn =
                field.getAnnotation(de.timscho.config.annotations.Element.class);
        final ElementValues elementValues = field.getAnnotation(ElementValues.class);
        final Range range = field.getAnnotation(Range.class);

        final int annotationCount =
                (elementAnn != null ? 1 : 0) + (elementValues != null ? 1 : 0) + (range != null ? 1 : 0);
        if (annotationCount > 1) {
            throw error(
                    field, "carries more than one of @Element/@ElementValues/@Range; exactly one describes an element");
        }

        if (elementAnn != null) {
            final TypeElement nestedType = requireTerraceConfigStruct(field, elementType, "@Element");
            final String nestedKeys = descriptorReference(nestedType) + ".DESCRIPTOR.keys()";
            result.element = "new de.timscho.config.core.descriptor.ElementDescriptor("
                    + CodeGen.stringLiteral(TypeNames.simplify(elementType.toString())) + ", "
                    + "java.util.List.of(), null, " + nestedKeys + ")";
            return result;
        }
        if (elementValues != null) {
            final String valuesExpr =
                    resolveValues(field, elementType, elementValues::from, elementValues.value(), "@ElementValues");
            result.element = "new de.timscho.config.core.descriptor.ElementDescriptor("
                    + CodeGen.stringLiteral(TypeNames.simplify(elementType.toString())) + ", "
                    + valuesExpr + ", null, java.util.List.of())";
            return result;
        }
        if (range != null) {
            requireNumericLeaf(field, elementType, "@Range");
            result.element = "new de.timscho.config.core.descriptor.ElementDescriptor("
                    + CodeGen.stringLiteral(TypeNames.simplify(elementType.toString())) + ", "
                    + "java.util.List.of(), " + rangeExpression(field, range) + ", java.util.List.of())";
            return result;
        }
        if (LeafTypes.isLeaf(elementType)) {
            return result;
        }
        throw error(
                field,
                "is a container whose element type " + elementType
                        + " publishes no shape; annotate it with @Element, @ElementValues, @Range, or @Skip the field");
    }

    private String resolveValues(
            final Element site,
            final TypeMirror ownType,
            final java.util.function.Supplier<Class<?>> fromAccessor,
            final String[] literal,
            final String attribute) {
        final TypeMirror fromMirror = mirrorOf(fromAccessor);
        final boolean fromSet = fromMirror != null && !fromMirror.toString().equals("java.lang.Void");
        final boolean literalSet = literal.length > 0;
        if (fromSet && literalSet) {
            throw error(site, attribute + " cannot combine from(...) with a literal list");
        }
        if (literalSet) {
            return CodeGen.stringListLiteral(List.of(literal));
        }
        final TypeMirror enumType = fromSet ? fromMirror : ownType;
        final TypeElement enumElement = requireTerraceConfigEnum(site, enumType, attribute);
        return descriptorReference(enumElement) + ".DESCRIPTOR.values()";
    }

    private String rangeExpression(final Element site, final Range range) {
        final Double min = finiteOrNull(range.min());
        final Double max = finiteOrNull(range.max());
        final Double exclMin = finiteOrNull(range.exclusiveMin());
        final Double exclMax = finiteOrNull(range.exclusiveMax());
        if (min == null && max == null && exclMin == null && exclMax == null) {
            throw error(site, "@Range has none of min/max/exclusiveMin/exclusiveMax set");
        }
        return "new de.timscho.config.core.descriptor.RangeConstraint("
                + CodeGen.doubleLiteral(min) + ", " + CodeGen.doubleLiteral(max) + ", "
                + CodeGen.doubleLiteral(exclMin) + ", " + CodeGen.doubleLiteral(exclMax) + ")";
    }

    private static @Nullable Double finiteOrNull(final double value) {
        return Double.isNaN(value) ? null : value;
    }

    private TypeElement requireTerraceConfigStruct(final Element site, final TypeMirror type, final String attribute) {
        final TypeElement element = asDeclaredElement(type);
        if (element == null
                || element.getAnnotation(TerraceConfig.class) == null
                || element.getKind() == javax.lang.model.element.ElementKind.ENUM) {
            throw error(
                    site, attribute + " needs a type annotated @TerraceConfig as a struct; " + type + " is not one");
        }
        return element;
    }

    private TypeElement requireTerraceConfigEnum(final Element site, final TypeMirror type, final String attribute) {
        final TypeElement element = asDeclaredElement(type);
        if (element == null
                || element.getAnnotation(TerraceConfig.class) == null
                || element.getKind() != javax.lang.model.element.ElementKind.ENUM) {
            throw error(site, attribute + " needs an enum annotated @TerraceConfig; " + type + " is not one");
        }
        return element;
    }

    private void requireNumericLeaf(final Element site, final TypeMirror type, final String attribute) {
        if (!LeafTypes.isNumeric(type)) {
            throw error(site, attribute + " needs a numeric type; " + type + " is not one");
        }
    }

    private @Nullable TypeElement asDeclaredElement(final TypeMirror type) {
        if (!(type instanceof DeclaredType)) {
            return null;
        }
        final javax.lang.model.element.Element element = ((DeclaredType) type).asElement();
        return element instanceof TypeElement ? (TypeElement) element : null;
    }

    private String descriptorReference(final TypeElement type) {
        return DescriptorNaming.qualifiedName(type, elements);
    }

    private DescriptorException mustSayNothingError(final Element field, final TypeMirror type) {
        return error(
                field,
                "publishes no shape at all; its type " + type
                        + " is not a recognised leaf. Resolve it with @Values, @Nested, @Range, or @Skip the field");
    }

    private DescriptorException error(final Element site, final String message) {
        return new DescriptorException(site, "field '" + site.getSimpleName() + "' " + message);
    }

    /** Reads an annotation's {@code Class} element via the standard {@link MirroredTypeException} dance. */
    private @Nullable TypeMirror mirrorOf(final java.util.function.Supplier<Class<?>> access) {
        try {
            access.get();
            return null;
        } catch (MirroredTypeException mte) {
            return mte.getTypeMirror();
        }
    }
}
