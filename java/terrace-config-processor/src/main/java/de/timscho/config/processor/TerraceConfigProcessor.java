package de.timscho.config.processor;

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

import de.timscho.config.annotations.TerraceConfig;

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
@SupportedSourceVersion(SourceVersion.RELEASE_21)
public final class TerraceConfigProcessor extends AbstractProcessor {

    private Messager messager;
    private Filer filer;
    private Elements elements;
    private FieldResolver fieldResolver;

    @Override
    public synchronized void init(javax.annotation.processing.ProcessingEnvironment env) {
        super.init(env);
        this.messager = env.getMessager();
        this.filer = env.getFiler();
        this.elements = env.getElementUtils();
        this.fieldResolver = new FieldResolver(env);
    }

    @Override
    public boolean process(Set<? extends TypeElement> annotations, RoundEnvironment roundEnv) {
        for (Element element : roundEnv.getElementsAnnotatedWith(TerraceConfig.class)) {
            if (!(element instanceof TypeElement)) {
                continue;
            }
            generate((TypeElement) element);
        }
        return true;
    }

    private void generate(TypeElement type) {
        List<DescriptorException> errors = new ArrayList<>();
        String source = type.getKind() == ElementKind.ENUM
                ? renderEnum(type)
                : renderStruct(type, errors);

        for (DescriptorException error : errors) {
            messager.printMessage(Diagnostic.Kind.ERROR, error.getMessage(), error.element());
        }
        if (!errors.isEmpty()) {
            return;
        }

        String descriptorName = DescriptorNaming.qualifiedName(type, elements);
        try {
            JavaFileObject file = filer.createSourceFile(descriptorName, type);
            try (Writer writer = file.openWriter()) {
                writer.write(source);
            }
        } catch (IOException e) {
            messager.printMessage(Diagnostic.Kind.ERROR,
                    "failed to write " + descriptorName + ": " + e.getMessage(), type);
        }
    }

    private String renderStruct(TypeElement type, List<DescriptorException> errors) {
        List<String> keyExpressions = new ArrayList<>();
        for (Element enclosed : type.getEnclosedElements()) {
            if (enclosed.getKind() != ElementKind.FIELD) {
                continue;
            }
            VariableElement field = (VariableElement) enclosed;
            if (field.getModifiers().contains(Modifier.STATIC)) {
                continue;
            }
            try {
                String keyExpression = fieldResolver.resolve(field);
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
        boolean closed = JacksonReflection.isClosed(type);
        return renderClass(type, "de.timscho.config.core.descriptor.TypeDescriptor.Kind.STRUCT",
                listOf(keyExpressions), "java.util.List.of()", closed);
    }

    private String renderEnum(TypeElement type) {
        List<String> values = new ArrayList<>();
        for (Element enclosed : type.getEnclosedElements()) {
            if (enclosed.getKind() != ElementKind.ENUM_CONSTANT) {
                continue;
            }
            String fallback = enclosed.getSimpleName().toString();
            values.add(JacksonReflection.jsonPropertyName(enclosed, fallback));
        }
        return renderClass(type, "de.timscho.config.core.descriptor.TypeDescriptor.Kind.ENUM",
                "java.util.List.of()", CodeGen.stringListLiteral(values), false);
    }

    private String renderClass(TypeElement type, String kindExpression, String keysExpression,
            String valuesExpression, boolean closed) {
        String pkg = elements.getPackageOf(type).getQualifiedName().toString();
        String simpleName = DescriptorNaming.simpleName(type);
        StringBuilder out = new StringBuilder();
        if (!pkg.isEmpty()) {
            out.append("package ").append(pkg).append(";\n\n");
        }
        out.append("/** Generated by terrace-config-processor from {@link ")
                .append(type.getQualifiedName()).append("}. Do not edit. */\n");
        out.append("public final class ").append(simpleName).append(" {\n\n");
        out.append("    private ").append(simpleName).append("() {\n    }\n\n");
        out.append("    public static final de.timscho.config.core.descriptor.TypeDescriptor DESCRIPTOR =\n");
        out.append("        new de.timscho.config.core.descriptor.TypeDescriptor(\n");
        out.append("            ").append(CodeGen.stringLiteral(type.getQualifiedName().toString())).append(",\n");
        out.append("            ").append(kindExpression).append(",\n");
        out.append("            ").append(keysExpression).append(",\n");
        out.append("            ").append(valuesExpression).append(",\n");
        out.append("            ").append(closed).append(");\n");
        out.append("}\n");
        return out.toString();
    }

    private static String listOf(List<String> expressions) {
        if (expressions.isEmpty()) {
            return "java.util.List.of()";
        }
        StringBuilder out = new StringBuilder("java.util.List.of(\n");
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
