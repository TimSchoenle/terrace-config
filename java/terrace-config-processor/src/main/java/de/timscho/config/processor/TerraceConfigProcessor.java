package de.timscho.config.processor;

import de.timscho.config.annotations.TerraceConfig;
import java.io.IOException;
import java.io.Writer;
import java.util.ArrayList;
import java.util.List;
import java.util.Set;
import javax.annotation.processing.AbstractProcessor;
import javax.annotation.processing.Filer;
import javax.annotation.processing.Messager;
import javax.annotation.processing.RoundEnvironment;
import javax.annotation.processing.SupportedAnnotationTypes;
import javax.annotation.processing.SupportedSourceVersion;
import javax.lang.model.SourceVersion;
import javax.lang.model.element.Element;
import javax.lang.model.element.ElementKind;
import javax.lang.model.element.Modifier;
import javax.lang.model.element.TypeElement;
import javax.lang.model.element.VariableElement;
import javax.lang.model.util.Elements;
import javax.tools.Diagnostic;
import javax.tools.JavaFileObject;

/**
 * Generates, for each type annotated {@link TerraceConfig}, a sibling {@code <Type>Descriptor}
 * class exposing a {@code public static final TypeDescriptor DESCRIPTOR}. The Java answer to
 * {@code #[derive(Describe)]} — see {@code rust/docs/SCHEMA.md}.
 *
 * <p>A struct's fields become {@code KEYS}; an enum's constants become {@code VALUES}. Descriptors
 * are dialect-agnostic — no environment spelling, no {@code text_form} — a loader or Spring
 * producer combines one with its own dialect to build the actual {@code Key} model.
 */
@SupportedAnnotationTypes("de.timscho.config.annotations.TerraceConfig")
@SupportedSourceVersion(SourceVersion.RELEASE_25)
public final class TerraceConfigProcessor extends AbstractProcessor {

    private Messager messager;
    private Filer filer;
    private Elements elements;
    private FieldResolver fieldResolver;

    @Override
    public synchronized void init(final javax.annotation.processing.ProcessingEnvironment env) {
        super.init(env);
        this.messager = env.getMessager();
        this.filer = env.getFiler();
        this.elements = env.getElementUtils();
        this.fieldResolver = new FieldResolver(env);
    }

    @Override
    public boolean process(final Set<? extends TypeElement> annotations, final RoundEnvironment roundEnv) {
        for (final Element element : roundEnv.getElementsAnnotatedWith(TerraceConfig.class)) {
            if (!(element instanceof TypeElement)) {
                continue;
            }
            this.generate((TypeElement) element);
        }
        return true;
    }

    private void generate(final TypeElement type) {
        final List<DescriptorException> errors = new ArrayList<>();
        final String source =
                type.getKind() == ElementKind.ENUM ? this.renderEnum(type) : this.renderStruct(type, errors);

        for (final DescriptorException error : errors) {
            this.messager.printMessage(Diagnostic.Kind.ERROR, error.getMessage(), error.element());
        }
        if (!errors.isEmpty()) {
            return;
        }

        final String descriptorName = DescriptorNaming.qualifiedName(type, this.elements);
        try {
            final JavaFileObject file = this.filer.createSourceFile(descriptorName, type);
            try (Writer writer = file.openWriter()) {
                writer.write(source);
            }
        } catch (IOException e) {
            this.messager.printMessage(
                    Diagnostic.Kind.ERROR, "failed to write " + descriptorName + ": " + e.getMessage(), type);
        }
    }

    private String renderStruct(final TypeElement type, final List<DescriptorException> errors) {
        final List<String> keyExpressions = new ArrayList<>();
        for (final Element enclosed : type.getEnclosedElements()) {
            if (enclosed.getKind() != ElementKind.FIELD) {
                continue;
            }
            final VariableElement field = (VariableElement) enclosed;
            if (field.getModifiers().contains(Modifier.STATIC)) {
                continue;
            }
            try {
                final String keyExpression = this.fieldResolver.resolve(field);
                if (keyExpression != null) {
                    keyExpressions.add(keyExpression);
                }
            } catch (DescriptorException e) {
                errors.add(e);
            }
        }
        if (!errors.isEmpty()) {
            return "";
        }
        final boolean closed = JacksonReflection.isClosed(type);
        return this.renderClass(
                type,
                "de.timscho.config.core.descriptor.TypeDescriptor.Kind.STRUCT",
                listOf(keyExpressions),
                "java.util.List.of()",
                closed);
    }

    private String renderEnum(final TypeElement type) {
        final List<String> values = new ArrayList<>();
        for (final Element enclosed : type.getEnclosedElements()) {
            if (enclosed.getKind() != ElementKind.ENUM_CONSTANT) {
                continue;
            }
            final String fallback = enclosed.getSimpleName().toString();
            values.add(JacksonReflection.jsonPropertyName(enclosed, fallback));
        }
        return this.renderClass(
                type,
                "de.timscho.config.core.descriptor.TypeDescriptor.Kind.ENUM",
                "java.util.List.of()",
                CodeGen.stringListLiteral(values),
                false);
    }

    private String renderClass(
            final TypeElement type,
            final String kindExpression,
            final String keysExpression,
            final String valuesExpression,
            final boolean closed) {
        final String pkg = this.elements.getPackageOf(type).getQualifiedName().toString();
        final String simpleName = DescriptorNaming.simpleName(type);
        final StringBuilder out = new StringBuilder();
        if (!pkg.isEmpty()) {
            out.append("package ").append(pkg).append(";\n\n");
        }
        out.append("/** Generated by terrace-config-processor from {@link ")
                .append(type.getQualifiedName())
                .append("}. Do not edit. */\n");
        out.append("public final class ").append(simpleName).append(" {\n\n");
        out.append("    private ").append(simpleName).append("() {\n    }\n\n");
        out.append("    public static final de.timscho.config.core.descriptor.TypeDescriptor DESCRIPTOR =\n");
        out.append("        new de.timscho.config.core.descriptor.TypeDescriptor(\n");
        out.append("            ")
                .append(CodeGen.stringLiteral(type.getQualifiedName().toString()))
                .append(",\n");
        out.append("            ").append(kindExpression).append(",\n");
        out.append("            ").append(keysExpression).append(",\n");
        out.append("            ").append(valuesExpression).append(",\n");
        out.append("            ").append(closed).append(");\n");
        out.append("}\n");
        return out.toString();
    }

    private static String listOf(final List<String> expressions) {
        if (expressions.isEmpty()) {
            return "java.util.List.of()";
        }
        final StringBuilder out = new StringBuilder("java.util.List.of(\n");
        for (int i = 0; i < expressions.size(); i++) {
            if (i > 0) {
                out.append(",\n");
            }
            out.append("                ").append(expressions.get(i));
        }
        out.append(")");
        return out.toString();
    }
}
