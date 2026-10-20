package de.timscho.config.core.contract;

import de.timscho.config.core.model.Contract;
import de.timscho.config.core.refusal.ContractRefusalException;
import de.timscho.config.core.schema.RefinementException;
import java.io.IOException;
import java.io.PrintStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.LinkedHashSet;
import java.util.Set;
import java.util.function.Function;
import org.jetbrains.annotations.Blocking;

/**
 * What every build-time producer entry point shares: parsing, {@code --out}, and the exit status
 * a build reads.
 *
 * <p>The statuses are {@code terrace-contract}'s own, so a build script treats every step of
 * producing, stamping and conforming a contract the same way: {@code 0} the document was written,
 * {@code 1} the producer refused it ({@code spec/v1/FORMAT.md}'s "What a producer MUST refuse",
 * or a refinement that cannot be published), {@code 2} the command line or the classpath was
 * wrong, so nothing was examined at all.
 *
 * @param usage      the text printed for {@code --help} and after a usage error
 * @param single     the producer's own single-valued options, without {@code --}; {@code out} is
 *                   added here
 * @param repeatable the producer's own repeatable options, without {@code --}
 * @param producer   builds and validates the contract from the parsed options
 * @param codec      renders the contract as its {@code json} bytes
 */
public record ProducerCommandLine(
        String usage,
        Set<String> single,
        Set<String> repeatable,
        Function<ProducerArguments, Contract> producer,
        Function<Contract, byte[]> codec) {

    /** The document was written. */
    public static final int OK = 0;

    /** The producer refused to emit the document. */
    public static final int REFUSED = 1;

    /** The command line or the classpath was wrong; nothing was examined. */
    public static final int USAGE = 2;

    /** Where the document goes instead of standard output. */
    private static final String OUT = "out";

    /**
     * Runs the producer and writes its document to {@code --out}, or to {@code out} when that
     * option is absent.
     *
     * <p>Failures are reported on {@code err} and as the returned status rather than thrown, so a
     * {@code main} only has to pass the status to {@link System#exit} when it is not {@link #OK}.
     *
     * @param args the command line, as {@code main} received it
     * @param out  where the document goes without {@code --out}
     * @param err  where failures are reported
     * @return {@link #OK}, {@link #REFUSED} or {@link #USAGE}
     */
    @Blocking
    public int run(final String[] args, final PrintStream out, final PrintStream err) {
        if (args.length == 1 && ("--help".equals(args[0]) || "-h".equals(args[0]))) {
            out.print(this.usage);
            return OK;
        }
        final Set<String> options = new LinkedHashSet<>(this.single);
        options.add(OUT);
        final byte[] document;
        final ProducerArguments arguments;
        try {
            arguments = ProducerArguments.parse(args, options, this.repeatable);
            document = this.codec.apply(this.producer.apply(arguments));
        } catch (ContractRefusalException | RefinementException e) {
            err.println("error: refusing to emit this contract: " + e.getMessage());
            return REFUSED;
        } catch (IllegalArgumentException e) {
            err.println("error: " + e.getMessage());
            err.println();
            err.print(this.usage);
            return USAGE;
        }

        final String target = arguments.optional(OUT);
        if (target == null) {
            out.write(document, 0, document.length);
            out.flush();
            return OK;
        }
        try {
            final Path path = Path.of(target).toAbsolutePath();
            Files.createDirectories(path.getParent());
            Files.write(path, document);
        } catch (IOException e) {
            err.println("error: " + target + " could not be written: " + e.getMessage());
            return USAGE;
        }
        return OK;
    }
}
