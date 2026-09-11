package de.timscho.config.core.refusal;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatCode;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

import java.util.List;

import org.junit.jupiter.api.Test;

import de.timscho.config.core.model.App;
import de.timscho.config.core.model.Contract;
import de.timscho.config.core.model.Dialect;
import de.timscho.config.core.model.External;
import de.timscho.config.core.model.ExternalUnknownPolicy;
import de.timscho.config.core.model.ExternalVar;
import de.timscho.config.core.model.Key;
import de.timscho.config.core.model.LoaderRole;
import de.timscho.config.core.model.LoaderVar;
import de.timscho.config.core.model.Producer;
import de.timscho.config.core.model.Schema;
import de.timscho.config.core.model.TextForm;

/**
 * One test per {@code spec/v1/FORMAT.md} refusal, numbered as that section numbers them, plus one
 * proving a well-formed contract passes every check.
 */
class ContractValidatorTest {

    private static final Dialect DIALECT = Dialect.builder()
            .prefix("PORTFOLIO_")
            .nestingSeparator("__")
            .indirectionSuffix("_FILE")
            .build();

    private static final Producer PRODUCER = Producer.builder()
            .name("terrace-config-java")
            .version("0.1.0")
            .loader("terrace-java")
            .build();

    private static final App APP = App.builder().name("portfolio").build();

    private static Contract.ContractBuilder validContract() {
        return Contract.builder()
                .producer(PRODUCER)
                .app(APP)
                .schema(Schema.builder()
                        .schemaVersion(2)
                        .dialect(DIALECT)
                        .loader(List.of(loaderVar("PORTFOLIO_CONFIG", LoaderRole.CONFIG)))
                        .keys(List.of(key("dist_dir", "PORTFOLIO_DIST_DIR", "PORTFOLIO_DIST_DIR_FILE")))
                        .build())
                .jsonSchema(de.timscho.config.core.model.JsonSchemaDocument.of(
                        java.util.Map.of("$schema", "http://json-schema.org/draft-07/schema#")))
                .external(
                        External.builder().unknown(ExternalUnknownPolicy.REJECT).build());
    }

    private static LoaderVar loaderVar(String env, LoaderRole role) {
        return LoaderVar.builder().env(env).role(role).docs("").build();
    }

    private static Key key(String path, String env, String envFile) {
        return Key.builder()
                .path(path)
                .env(env)
                .envFile(envFile)
                .textForm(TextForm.TEXT)
                .build();
    }

    private static Key.KeyBuilder secretKey(String path) {
        return Key.builder().path(path).textForm(TextForm.TEXT).secret(true).unreachable(null);
    }

    @Test
    void wellFormedContractPassesEveryCheck() {
        assertThatCode(() -> ContractValidator.validate(validContract().build()))
                .doesNotThrowAnyException();
    }

    @Test
    void refusal1_externalVariableInPrefix() {
        Contract contract = validContract()
                .external(External.builder()
                        .env(List.of(ExternalVar.builder()
                                .name("PORTFOLIO_SECRET")
                                .textForm(TextForm.TEXT)
                                .build()))
                        .unknown(ExternalUnknownPolicy.REJECT)
                        .build())
                .build();

        assertThatThrownBy(() -> ContractValidator.validate(contract))
                .isInstanceOf(ExternalVariableInPrefixException.class);
    }

    @Test
    void refusal2_ignorePatternInPrefix() {
        Contract contract = validContract()
                .external(External.builder()
                        .ignore(List.of("PORT*"))
                        .unknown(ExternalUnknownPolicy.REJECT)
                        .build())
                .build();

        assertThatThrownBy(() -> ContractValidator.validate(contract))
                .isInstanceOf(IgnorePatternInPrefixException.class);
    }

    @Test
    void refusal2_exactNameOutsideNamespaceIsFine() {
        Contract contract = validContract()
                .external(External.builder()
                        .ignore(List.of("PORT"))
                        .unknown(ExternalUnknownPolicy.REJECT)
                        .build())
                .build();

        assertThatCode(() -> ContractValidator.validate(contract)).doesNotThrowAnyException();
    }

    @Test
    void refusal3_externalVariableCollidesWithLoaderVariable() {
        // `CREDENTIALS_DIR` again does not carry `PORTFOLIO_`, so this is refusal #3 reached
        // through a reserved/loader name, not #1 reached a second way.
        Contract base = validContract().build();
        Contract contract = base.toBuilder()
                .schema(base.getSchema().toBuilder()
                        .loader(List.of(loaderVar("CREDENTIALS_DIR", LoaderRole.SECRETS_DIR)))
                        .build())
                .external(External.builder()
                        .env(List.of(ExternalVar.builder()
                                .name("CREDENTIALS_DIR")
                                .textForm(TextForm.TEXT)
                                .build()))
                        .unknown(ExternalUnknownPolicy.REJECT)
                        .build())
                .build();

        assertThatThrownBy(() -> ContractValidator.validate(contract))
                .isInstanceOf(ExternalVariableCollisionException.class);
    }

    @Test
    void refusal4_ignorePatternCoversLoaderVariable() {
        // `CREDENTIALS_DIR` deliberately does not carry `PORTFOLIO_`, mirroring FORMAT.md's own
        // example: the secrets-directory variable takes an arbitrary name, so a pattern covering
        // it is a distinct refusal from #2, not the same one reached a second way.
        Contract base = validContract().build();
        Contract contract = base.toBuilder()
                .schema(base.getSchema().toBuilder()
                        .loader(List.of(loaderVar("CREDENTIALS_DIR", LoaderRole.SECRETS_DIR)))
                        .build())
                .external(External.builder()
                        .ignore(List.of("CREDENTIALS_*"))
                        .unknown(ExternalUnknownPolicy.REJECT)
                        .build())
                .build();

        assertThatThrownBy(() -> ContractValidator.validate(contract))
                .isInstanceOf(IgnorePatternCollisionException.class);
    }

    @Test
    void refusal5_duplicateExternalVariable() {
        ExternalVar port =
                ExternalVar.builder().name("PORT").textForm(TextForm.TEXT).build();
        Contract contract = validContract()
                .external(External.builder()
                        .env(List.of(port, port))
                        .unknown(ExternalUnknownPolicy.REJECT)
                        .build())
                .build();

        assertThatThrownBy(() -> ContractValidator.validate(contract))
                .isInstanceOf(DuplicateExternalVariableException.class);
    }

    @Test
    void refusal6_secretKeyCarriesADefault() {
        Contract contract = validContract()
                .schema(validContract().build().getSchema().toBuilder()
                        .keys(List.of(secretKey("github.token").defaultText("x").build()))
                        .build())
                .build();

        assertThatThrownBy(() -> ContractValidator.validate(contract)).isInstanceOf(SecretWithDefaultException.class);
    }

    @Test
    void refusal7_emptyPrefix() {
        Contract contract = validContract()
                .schema(validContract().build().getSchema().toBuilder()
                        .dialect(DIALECT.toBuilder().prefix("").build())
                        .build())
                .build();

        assertThatThrownBy(() -> ContractValidator.validate(contract)).isInstanceOf(EmptyPrefixException.class);
    }

    @Test
    void refusal8_keyEnvIsAnotherKeysIndirectionVariable() {
        Key token = key("github.token", "PORTFOLIO_GITHUB__TOKEN", "PORTFOLIO_GITHUB__TOKEN_FILE");
        Key collider = key("github.token_file", "PORTFOLIO_GITHUB__TOKEN_FILE", null);
        Contract contract = validContract()
                .schema(validContract().build().getSchema().toBuilder()
                        .keys(List.of(token, collider))
                        .build())
                .build();

        assertThatThrownBy(() -> ContractValidator.validate(contract))
                .isInstanceOf(IndirectionCollisionException.class);
    }

    @Test
    void exceptionMessagesNameTheOffendingField() {
        assertThat(new EmptyPrefixException()).hasMessageContaining("prefix");
    }
}
