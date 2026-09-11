package de.timscho.config.processor;

import java.util.List;

import javax.annotation.processing.ProcessingEnvironment;
import javax.lang.model.element.Element;
import javax.lang.model.element.TypeElement;
import javax.lang.model.element.VariableElement;
import javax.lang.model.type.DeclaredType;
import javax.lang.model.type.MirroredTypeException;
import javax.lang.model.type.TypeMirror;
import javax.lang.model.util.Elements;

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

    FieldResolver(ProcessingEnvironment env) {
        this.elements = env.getElementUtils();
    }

    /** {@code null} means the field carries {@code @Skip} and contributes no key. */
    String resolve(VariableElement field) {
        if (field.getAnnotation(de.timscho.config.annotations.Skip.class) != null) {
            return null;
        }

        String name = field.getSimpleName().toString();
        String docs = Javadocs.normalize(elements.getDocComment(field));
        String summary = Javadocs.summary(docs);
        boolean secret = field.getAnnotation(Secret.class) != null;
        Note noteAnn = field.getAnnotation(Note.class);
        String note = noteAnn == null ? null : noteAnn.value();

        TypeMirror declaredType = field.asType();
        String typeName = TypeNames.simplify(declaredType.toString());
        ContainerShape shape = ContainerShape.of(declaredType);

        Shape resolved = shape.kind() == ContainerKind.NONE
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
                + resolved.closed
                + ")";
    }

    private static final class Shape {
        String values = "java.util.List.of()";
        String range = "null";
        String nestedKeys = "java.util.List.of()";
        String element = "null";
        boolean closed;
    }

    private Shape resolveLeafField(VariableElement field, TypeMirror declaredType) {
        Shape shape = new Shape();
        Nested nested = field.getAnnotation(Nested.class);
        Values values = field.getAnnotation(Values.class);
        Range range = field.getAnnotation(Range.class);

        int annotationCount = (nested != null ? 1 : 0) + (values != null ? 1 : 0) + (range != null ? 1 : 0);
        if (annotationCount > 1) {
            throw error(field, "carries more than one of @Nested/@Values/@Range; exactly one describes a field");
        }

        if (nested != null) {
            TypeElement nestedType = requireTerraceConfigStruct(field, declaredType, "@Nested");
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

    private Shape resolveContainerField(VariableElement field, ContainerShape shape) {
        Shape result = new Shape();
        TypeMirror elementType = shape.element();

        if (field.getAnnotation(Nested.class) != null || field.getAnnotation(Values.class) != null) {
            throw error(field, "is a container field; use @Element/@ElementValues, not @Nested/@Values");
        }

        de.timscho.config.annotations.Element elementAnn =
                field.getAnnotation(de.timscho.config.annotations.Element.class);
        ElementValues elementValues = field.getAnnotation(ElementValues.class);
        Range range = field.getAnnotation(Range.class);

        int annotationCount = (elementAnn != null ? 1 : 0) + (elementValues != null ? 1 : 0) + (range != null ? 1 : 0);
        if (annotationCount > 1) {
            throw error(field, "carries more than one of @Element/@ElementValues/@Range; exactly one describes an element");
        }

        if (elementAnn != null) {
            TypeElement nestedType = requireTerraceConfigStruct(field, elementType, "@Element");
            String nestedKeys = descriptorReference(nestedType) + ".DESCRIPTOR.keys()";
            result.element = "new de.timscho.config.core.descriptor.ElementDescriptor("
                    + CodeGen.stringLiteral(TypeNames.simplify(elementType.toString())) + ", "
                    + "java.util.List.of(), null, " + nestedKeys + ")";
            return result;
        }
        if (elementValues != null) {
            String valuesExpr = resolveValues(field, elementType, elementValues::from, elementValues.value(), "@ElementValues");
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
        throw error(field, "is a container whose element type " + elementType
                + " publishes no shape; annotate it with @Element, @ElementValues, @Range, or @Skip the field");
    }

    private String resolveValues(Element site, TypeMirror ownType, java.util.function.Supplier<Class<?>> fromAccessor,
            String[] literal, String attribute) {
        TypeMirror fromMirror = mirrorOf(fromAccessor);
        boolean fromSet = fromMirror != null && !fromMirror.toString().equals("java.lang.Void");
        boolean literalSet = literal.length > 0;
        if (fromSet && literalSet) {
            throw error(site, attribute + " cannot combine from(...) with a literal list");
        }
        if (literalSet) {
            return CodeGen.stringListLiteral(List.of(literal));
        }
        TypeMirror enumType = fromSet ? fromMirror : ownType;
        TypeElement enumElement = requireTerraceConfigEnum(site, enumType, attribute);
        return descriptorReference(enumElement) + ".DESCRIPTOR.values()";
    }

    private String rangeExpression(Element site, Range range) {
        Double min = finiteOrNull(range.min());
        Double max = finiteOrNull(range.max());
        Double exclMin = finiteOrNull(range.exclusiveMin());
        Double exclMax = finiteOrNull(range.exclusiveMax());
        if (min == null && max == null && exclMin == null && exclMax == null) {
            throw error(site, "@Range has none of min/max/exclusiveMin/exclusiveMax set");
        }
        return "new de.timscho.config.core.descriptor.RangeConstraint("
                + CodeGen.doubleLiteral(min) + ", " + CodeGen.doubleLiteral(max) + ", "
                + CodeGen.doubleLiteral(exclMin) + ", " + CodeGen.doubleLiteral(exclMax) + ")";
    }

    private static Double finiteOrNull(double value) {
        return Double.isNaN(value) ? null : value;
    }

    private TypeElement requireTerraceConfigStruct(Element site, TypeMirror type, String attribute) {
        TypeElement element = asDeclaredElement(type);
        if (element == null || element.getAnnotation(TerraceConfig.class) == null
                || element.getKind() == javax.lang.model.element.ElementKind.ENUM) {
            throw error(site, attribute + " needs a type annotated @TerraceConfig as a struct; " + type + " is not one");
        }
        return element;
    }

    private TypeElement requireTerraceConfigEnum(Element site, TypeMirror type, String attribute) {
        TypeElement element = asDeclaredElement(type);
        if (element == null || element.getAnnotation(TerraceConfig.class) == null
                || element.getKind() != javax.lang.model.element.ElementKind.ENUM) {
            throw error(site, attribute + " needs an enum annotated @TerraceConfig; " + type + " is not one");
        }
        return element;
    }

    private void requireNumericLeaf(Element site, TypeMirror type, String attribute) {
        if (!LeafTypes.isNumeric(type)) {
            throw error(site, attribute + " needs a numeric type; " + type + " is not one");
        }
    }

    private TypeElement asDeclaredElement(TypeMirror type) {
        if (!(type instanceof DeclaredType)) {
            return null;
        }
        javax.lang.model.element.Element element = ((DeclaredType) type).asElement();
        return element instanceof TypeElement ? (TypeElement) element : null;
    }

    private String descriptorReference(TypeElement type) {
        return DescriptorNaming.qualifiedName(type, elements);
    }

    private DescriptorException mustSayNothingError(Element field, TypeMirror type) {
        return error(field, "publishes no shape at all; its type " + type
                + " is not a recognised leaf. Resolve it with @Values, @Nested, @Range, or @Skip the field");
    }

    private DescriptorException error(Element site, String message) {
        return new DescriptorException(site, "field '" + site.getSimpleName() + "' " + message);
    }

    /** Reads an annotation's {@code Class} element via the standard {@link MirroredTypeException} dance. */
    private TypeMirror mirrorOf(java.util.function.Supplier<Class<?>> access) {
        try {
            access.get();
            return null;
        } catch (MirroredTypeException mte) {
            return mte.getTypeMirror();
        }
    }
}
