package de.timscho.config.core.contract;

import java.util.List;
import java.util.Map;

import org.junit.jupiter.api.Test;

import de.timscho.config.core.model.App;
import de.timscho.config.core.model.Contract;
import de.timscho.config.core.model.Dialect;
import de.timscho.config.core.model.External;
import de.timscho.config.core.model.ExternalUnknownPolicy;
import de.timscho.config.core.model.ExternalVar;
import de.timscho.config.core.model.Key;
import de.timscho.config.core.model.LabelFault;
import de.timscho.config.core.model.LoaderRole;
import de.timscho.config.core.model.LoaderVar;
import de.timscho.config.core.model.Producer;
import de.timscho.config.core.model.Schema;
import de.timscho.config.core.model.TextForm;
import de.timscho.config.core.refusal.EmptyPrefixException;
import de.timscho.config.core.refusal.ExternalVariableInPrefixException;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

/** Assembling a full {@link Contract} from a built {@link Schema}, an {@link App} and a {@link Producer}. */
class ContractAssemblerTest {

    private static final Dialect DIALECT = Dialect.builder()
            .prefix("PORTFOLIO_")
            .nestingSeparator("__")
            .indirectionSuffix("_FILE")
            .build();

    private static final App APP = App.builder().name("portfolio").version("v2.5.0").build();

    private static Schema schema() {
        return Schema.builder()
                .schemaVersion(2)
                .dialect(DIALECT)
                .loader(List.of(LoaderVar.builder()
                        .env("PORTFOLIO_CONFIG")
                        .role(LoaderRole.CONFIG)
                        .docs("")
                        .build()))
                .keys(List.of(Key.builder()
                        .path("dist_dir")
                        .env("PORTFOLIO_DIST_DIR")
                        .textForm(TextForm.TEXT)
                        .build()))
                .build();
    }

    @Test
    void assemblesAWellFormedContract() {
        Contract contract = ContractAssembler.assemble(schema(), APP, ProducerIdentity.forLoader("terrace-java"));

        assertThat(contract.getTerraceContract()).isEqualTo(Contract.ENVELOPE_VERSION);
        assertThat(contract.getProducer().getName()).isEqualTo(ProducerIdentity.NAME);
        assertThat(contract.getApp()).isEqualTo(APP);
        assertThat(contract.getSchema()).isEqualTo(schema());
        assertThat(contract.getJsonSchema().schemaDialect())
                .isEqualTo("http://json-schema.org/draft-07/schema#");
        assertThat(contract.getExternal().getUnknown()).isEqualTo(ExternalUnknownPolicy.REJECT);
    }

    @Test
    void jsonSchemaTitleDefaultsToTheAppNameConfiguration() {
        Contract contract = ContractAssembler.assemble(schema(), APP, ProducerIdentity.forLoader("terrace-java"));

        assertThat(contract.getJsonSchema().asMap()).containsEntry("title", "portfolio configuration");
    }

    @Test
    void derivesAMissingExternalVariableConstraintFromItsType() {
        External external = External.builder()
                .env(List.of(ExternalVar.builder()
                        .name("PORT")
                        .ty("int")
                        .textForm(TextForm.INTEGER)
                        .build()))
                .unknown(ExternalUnknownPolicy.REJECT)
                .build();

        Contract contract = ContractAssembler.assemble(
                schema(), APP, ProducerIdentity.forLoader("terrace-java"), external);

        assertThat(contract.getExternal().getEnv()).hasSize(1);
        assertThat(contract.getExternal().getEnv().get(0).getConstraint())
                .containsEntry("type", "integer");
    }

    @Test
    void leavesAnAlreadyStatedExternalVariableConstraintAlone() {
        Map<String, Object> handWritten = Map.of("type", "string", "pattern", "^[0-9]+$");
        External external = External.builder()
                .env(List.of(ExternalVar.builder()
                        .name("PORT")
                        .textForm(TextForm.TEXT)
                        .constraint(handWritten)
                        .build()))
                .unknown(ExternalUnknownPolicy.REJECT)
                .build();

        Contract contract = ContractAssembler.assemble(
                schema(), APP, ProducerIdentity.forLoader("terrace-java"), external);

        assertThat(contract.getExternal().getEnv().get(0).getConstraint()).isEqualTo(handWritten);
    }

    @Test
    void refusesAnExternalVariableCarryingThePrefix() {
        External external = External.builder()
                .env(List.of(ExternalVar.builder().name("PORTFOLIO_SECRET").textForm(TextForm.TEXT).build()))
                .unknown(ExternalUnknownPolicy.REJECT)
                .build();

        assertThatThrownBy(() -> ContractAssembler.assemble(
                schema(), APP, ProducerIdentity.forLoader("terrace-java"), external))
                .isInstanceOf(ExternalVariableInPrefixException.class);
    }

    @Test
    void refusesAnEmptyPrefix() {
        Schema emptyPrefix = schema().toBuilder()
                .dialect(DIALECT.toBuilder().prefix("").build())
                .build();

        assertThatThrownBy(() -> ContractAssembler.assemble(
                emptyPrefix, APP, ProducerIdentity.forLoader("terrace-java")))
                .isInstanceOf(EmptyPrefixException.class);
    }

    @Test
    void labelsNameTheVersionPathAndPrefix() {
        Contract contract = ContractAssembler.assemble(schema(), APP, ProducerIdentity.forLoader("terrace-java"));

        assertThat(contract.labels("/config/contract.json")).containsExactly(
                Map.entry(Contract.LABEL_VERSION, "1"),
                Map.entry(Contract.LABEL_PATH, "/config/contract.json"),
                Map.entry(Contract.LABEL_PREFIX, "PORTFOLIO_"));
    }

    @Test
    void dockerfileBlockIsWrappedInMarkers() {
        Contract contract = ContractAssembler.assemble(schema(), APP, ProducerIdentity.forLoader("terrace-java"));

        String block = contract.toDockerfileBlock(Contract.DEFAULT_PATH);

        assertThat(block).startsWith(Contract.MARKER_BEGIN + "\n");
        assertThat(block).endsWith(Contract.MARKER_END + "\n");
        assertThat(block).contains("LABEL " + Contract.LABEL_VERSION + "=\"1\"");
    }

    @Test
    void verifyLabelsReportsEveryFaultAtOnce() {
        Contract contract = ContractAssembler.assemble(schema(), APP, ProducerIdentity.forLoader("terrace-java"));

        List<LabelFault> faults = contract.checkLabels("/config/contract.json", Map.of());

        assertThat(faults).hasSize(3);
        assertThat(faults).allMatch(fault -> fault instanceof LabelFault.Missing);
        assertThatThrownBy(() -> contract.verifyLabels("/config/contract.json", Map.of()))
                .isInstanceOf(de.timscho.config.core.model.ContractLabelException.class)
                .satisfies(exception -> assertThat(
                        ((de.timscho.config.core.model.ContractLabelException) exception).getFaults()).hasSize(3));
    }

    @Test
    void verifyLabelsPassesWhenTheImageCarriesThemAll() {
        Contract contract = ContractAssembler.assemble(schema(), APP, ProducerIdentity.forLoader("terrace-java"));
        Map<String, String> imageLabels = Map.of(
                Contract.LABEL_VERSION, "1",
                Contract.LABEL_PATH, Contract.DEFAULT_PATH,
                Contract.LABEL_PREFIX, "PORTFOLIO_");

        contract.verifyLabels(Contract.DEFAULT_PATH, imageLabels);
    }
}
