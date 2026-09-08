package de.timscho.config.core.refusal;

import java.util.HashSet;
import java.util.Set;

import de.timscho.config.core.model.Contract;
import de.timscho.config.core.model.Dialect;
import de.timscho.config.core.model.External;
import de.timscho.config.core.model.ExternalVar;
import de.timscho.config.core.model.Key;
import de.timscho.config.core.model.LoaderVar;
import de.timscho.config.core.model.Schema;

/**
 * Runs {@code spec/v1/FORMAT.md}'s "What a producer MUST refuse" against a built {@link Contract},
 * throwing the first {@link ContractRefusalException} it finds.
 *
 * <p>This belongs beside the model rather than inside a loader: both {@code -loader} and
 * {@code -spring-boot} build the same {@link Contract} shape and must refuse the same eight
 * things, so the check is written once here rather than twice downstream.
 *
 * <p>No streams, per the project's convention — every scan below is a plain loop.
 */
public final class ContractValidator {

    private ContractValidator() {
    }

    /**
     * @throws ContractRefusalException the first refusal found, checked in the order
     *                                   {@code FORMAT.md} lists them
     */
    public static void validate(Contract contract) {
        Schema schema = contract.getSchema();
        Dialect dialect = schema.getDialect();
        External external = contract.getExternal();

        checkEmptyPrefix(dialect);
        checkExternalVariablesInPrefix(external, dialect);
        checkIgnorePatternsInPrefix(external, dialect);
        checkExternalVariableCollisions(external, schema);
        checkIgnorePatternCollisions(external, schema);
        checkDuplicateExternalVariables(external);
        checkSecretsWithDefaults(schema, external);
        checkIndirectionCollisions(schema);
    }

    /** Refusal 7. Checked first: every other check assumes a namespace actually exists. */
    private static void checkEmptyPrefix(Dialect dialect) {
        if (dialect.getPrefix().isEmpty()) {
            throw new EmptyPrefixException();
        }
    }

    /** Refusal 1. */
    private static void checkExternalVariablesInPrefix(External external, Dialect dialect) {
        for (ExternalVar var : external.getEnv()) {
            if (var.getName().startsWith(dialect.getPrefix())) {
                throw new ExternalVariableInPrefixException(var.getName(), dialect.getPrefix());
            }
        }
    }

    /** Refusal 2. */
    private static void checkIgnorePatternsInPrefix(External external, Dialect dialect) {
        String prefix = dialect.getPrefix();
        for (String pattern : external.getIgnore()) {
            String literal = literalPart(pattern);
            boolean namesInsideNamespace = literal.startsWith(prefix);
            boolean wildcardSubsumesNamespace = isWildcard(pattern) && prefix.startsWith(literal);
            if (namesInsideNamespace || wildcardSubsumesNamespace) {
                throw new IgnorePatternInPrefixException(pattern, prefix);
            }
        }
    }

    /** Refusal 3. */
    private static void checkExternalVariableCollisions(External external, Schema schema) {
        for (ExternalVar var : external.getEnv()) {
            for (LoaderVar loaderVar : schema.getLoader()) {
                if (var.getName().equals(loaderVar.getEnv())) {
                    throw new ExternalVariableCollisionException(var.getName());
                }
            }
        }
    }

    /** Refusal 4. */
    private static void checkIgnorePatternCollisions(External external, Schema schema) {
        for (String pattern : external.getIgnore()) {
            for (LoaderVar loaderVar : schema.getLoader()) {
                if (matches(pattern, loaderVar.getEnv())) {
                    throw new IgnorePatternCollisionException(pattern, loaderVar.getEnv());
                }
            }
        }
    }

    /** Refusal 5. */
    private static void checkDuplicateExternalVariables(External external) {
        Set<String> seen = new HashSet<>();
        for (ExternalVar var : external.getEnv()) {
            if (!seen.add(var.getName())) {
                throw new DuplicateExternalVariableException(var.getName());
            }
        }
    }

    /** Refusal 6, over both keys and external variables. */
    private static void checkSecretsWithDefaults(Schema schema, External external) {
        for (Key key : schema.getKeys()) {
            if (key.isSecret() && (key.getDefaultText() != null || key.getDefaultValue() != null)) {
                throw new SecretWithDefaultException(key.getPath());
            }
        }
        for (ExternalVar var : external.getEnv()) {
            if (var.isSecret() && var.getDefaultText() != null) {
                throw new SecretWithDefaultException(var.getName());
            }
        }
    }

    /** Refusal 8. */
    private static void checkIndirectionCollisions(Schema schema) {
        for (Key key : schema.getKeys()) {
            if (key.getEnv() == null) {
                continue;
            }
            for (Key other : schema.getKeys()) {
                if (other == key) {
                    continue;
                }
                if (key.getEnv().equals(other.getEnvFile())) {
                    throw new IndirectionCollisionException(key.getPath(), other.getPath(), key.getEnv());
                }
            }
        }
    }

    private static boolean isWildcard(String pattern) {
        return pattern.endsWith("*");
    }

    private static String literalPart(String pattern) {
        return isWildcard(pattern) ? pattern.substring(0, pattern.length() - 1) : pattern;
    }

    /** Whether {@code name} is covered by {@code pattern}, where only a trailing {@code *} is a wildcard. */
    private static boolean matches(String pattern, String name) {
        if (isWildcard(pattern)) {
            return name.startsWith(literalPart(pattern));
        }
        return pattern.equals(name);
    }
}
