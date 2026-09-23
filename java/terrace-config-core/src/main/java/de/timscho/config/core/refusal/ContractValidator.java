package de.timscho.config.core.refusal;

import de.timscho.config.core.model.Contract;
import de.timscho.config.core.model.Dialect;
import de.timscho.config.core.model.External;
import de.timscho.config.core.model.ExternalVar;
import de.timscho.config.core.model.Key;
import de.timscho.config.core.model.LoaderVar;
import de.timscho.config.core.model.Reload;
import de.timscho.config.core.model.ReloadMode;
import de.timscho.config.core.model.ReloadSupport;
import de.timscho.config.core.model.Schema;
import java.util.HashSet;
import java.util.Map;
import java.util.Set;

/**
 * Runs {@code spec/v1/FORMAT.md}'s "What a producer MUST refuse" against a built {@link Contract},
 * throwing the first {@link ContractRefusalException} it finds.
 *
 * <p>This belongs beside the model rather than inside a loader: both {@code -loader} and
 * {@code -spring-boot} build the same {@link Contract} shape and must refuse the same twelve
 * things, so the check is written once here rather than twice downstream.
 *
 * <p>No streams, per the project's convention — every scan below is a plain loop.
 */
public final class ContractValidator {

    private ContractValidator() {}

    /**
     * Throws on the first refusal found in the contract.
     *
     * @param contract the contract to validate
     * @throws ContractRefusalException the first refusal found, checked in the order
     *                                   {@code FORMAT.md} lists them
     */
    public static void validate(final Contract contract) {
        final Schema schema = contract.getSchema();
        final Dialect dialect = schema.getDialect();
        final External external = contract.getExternal();

        checkEmptyPrefix(dialect);
        checkExternalVariablesInPrefix(external, dialect);
        checkIgnorePatternsInPrefix(external, dialect);
        checkExternalVariableCollisions(external, schema);
        checkIgnorePatternCollisions(external, schema);
        checkDuplicateExternalVariables(external);
        checkSecretsWithDefaults(schema, external);
        checkIndirectionCollisions(schema);
        checkDefaultsAgainstConstraints(schema);
        checkReload(schema);
    }

    /** Refusal 7. Checked first: every other check assumes a namespace actually exists. */
    private static void checkEmptyPrefix(final Dialect dialect) {
        if (dialect.getPrefix().isEmpty()) {
            throw new EmptyPrefixException();
        }
    }

    /** Refusal 1. */
    private static void checkExternalVariablesInPrefix(final External external, final Dialect dialect) {
        for (final ExternalVar var : external.getEnv()) {
            if (var.getName().startsWith(dialect.getPrefix())) {
                throw new ExternalVariableInPrefixException(var.getName(), dialect.getPrefix());
            }
        }
    }

    /** Refusal 2. */
    private static void checkIgnorePatternsInPrefix(final External external, final Dialect dialect) {
        final String prefix = dialect.getPrefix();
        for (final String pattern : external.getIgnore()) {
            final String literal = literalPart(pattern);
            final boolean namesInsideNamespace = literal.startsWith(prefix);
            final boolean wildcardSubsumesNamespace = isWildcard(pattern) && prefix.startsWith(literal);
            if (namesInsideNamespace || wildcardSubsumesNamespace) {
                throw new IgnorePatternInPrefixException(pattern, prefix);
            }
        }
    }

    /** Refusal 3. */
    private static void checkExternalVariableCollisions(final External external, final Schema schema) {
        for (final ExternalVar var : external.getEnv()) {
            for (final LoaderVar loaderVar : schema.getLoader()) {
                if (var.getName().equals(loaderVar.getEnv())) {
                    throw new ExternalVariableCollisionException(var.getName());
                }
            }
        }
    }

    /** Refusal 4. */
    private static void checkIgnorePatternCollisions(final External external, final Schema schema) {
        for (final String pattern : external.getIgnore()) {
            for (final LoaderVar loaderVar : schema.getLoader()) {
                if (matches(pattern, loaderVar.getEnv())) {
                    throw new IgnorePatternCollisionException(pattern, loaderVar.getEnv());
                }
            }
        }
    }

    /** Refusal 5. */
    private static void checkDuplicateExternalVariables(final External external) {
        final Set<String> seen = new HashSet<>();
        for (final ExternalVar var : external.getEnv()) {
            if (!seen.add(var.getName())) {
                throw new DuplicateExternalVariableException(var.getName());
            }
        }
    }

    /** Refusal 6, over both keys and external variables. */
    private static void checkSecretsWithDefaults(final Schema schema, final External external) {
        for (final Key key : schema.getKeys()) {
            if (key.isSecret() && (key.getDefaultText() != null || key.getDefaultValue() != null)) {
                throw new SecretWithDefaultException(key.getPath());
            }
        }
        for (final ExternalVar var : external.getEnv()) {
            if (var.isSecret() && var.getDefaultText() != null) {
                throw new SecretWithDefaultException(var.getName());
            }
        }
    }

    /** Refusal 8. */
    private static void checkIndirectionCollisions(final Schema schema) {
        for (final Key key : schema.getKeys()) {
            if (key.getEnv() == null) {
                continue;
            }
            for (final Key other : schema.getKeys()) {
                if (other == key) {
                    continue;
                }
                if (key.getEnv().equals(other.getEnvFile())) {
                    throw new IndirectionCollisionException(key.getPath(), other.getPath(), key.getEnv());
                }
            }
        }
    }

    /**
     * Refusal 9: a key's {@code default_value} fails its own {@code constraint}. Only a failure
     * {@link ConstraintEvaluator} can prove is refused; a keyword it does not evaluate leaves the
     * default published rather than refused on a guess.
     */
    private static void checkDefaultsAgainstConstraints(final Schema schema) {
        for (final Key key : schema.getKeys()) {
            final Map<String, Object> constraint = key.getConstraint();
            final Object defaultValue = key.getDefaultValue();
            if (constraint == null || defaultValue == null) {
                continue;
            }
            if (ConstraintEvaluator.verdict(constraint, defaultValue) instanceof ConstraintEvaluator.Fails failed) {
                throw new DefaultViolatesConstraintException(
                        key.getPath(), ConstraintEvaluator.show(defaultValue), failed.reason());
            }
        }
    }

    /**
     * Refusals 10, 11 and 12: reload claims the rest of the same document contradicts. A consumer
     * leaves a {@code live} key out of the digest a chart rolls its pods on, so each of these is a
     * change a deployment would report as applied and the process would never apply.
     */
    private static void checkReload(final Schema schema) {
        final ReloadSupport support = schema.getReload();
        if (support != null
                && support.getMode() == ReloadMode.REBUILD
                && support.getLayers().isEmpty()) {
            throw new RebuildWithoutLayersException();
        }
        final boolean rebuilds = support != null && support.getMode() == ReloadMode.REBUILD;
        for (final Key key : schema.getKeys()) {
            if (key.getReload() != Reload.LIVE) {
                continue;
            }
            if (key.isReserved()) {
                throw new ReservedLiveException(key.getPath());
            }
            if (!rebuilds) {
                throw new LiveWithoutRebuildException(key.getPath());
            }
        }
    }

    private static boolean isWildcard(final String pattern) {
        return pattern.endsWith("*");
    }

    private static String literalPart(final String pattern) {
        return isWildcard(pattern) ? pattern.substring(0, pattern.length() - 1) : pattern;
    }

    /** Whether {@code name} is covered by {@code pattern}, where only a trailing {@code *} is a wildcard. */
    private static boolean matches(final String pattern, final String name) {
        if (isWildcard(pattern)) {
            return name.startsWith(literalPart(pattern));
        }
        return pattern.equals(name);
    }
}
