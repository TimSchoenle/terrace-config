package de.timscho.config.loader;

import static org.assertj.core.api.Assertions.assertThat;

import java.io.IOException;
import org.junit.jupiter.api.Test;

/** What each constructor of {@link LoaderException} records as its message and cause. */
class LoaderExceptionTest {

    @Test
    void the_no_argument_constructor_records_neither_message_nor_cause() {
        LoaderException exception = new LoaderException();

        assertThat(exception.getMessage()).isNull();
        assertThat(exception.getCause()).isNull();
    }

    @Test
    void the_message_constructor_records_no_cause() {
        LoaderException exception = new LoaderException("bad value");

        assertThat(exception.getMessage()).isEqualTo("bad value");
        assertThat(exception.getCause()).isNull();
    }

    @Test
    void the_cause_constructor_takes_the_causes_message() {
        IOException cause = new IOException("unreadable");
        LoaderException exception = new LoaderException(cause);

        assertThat(exception.getMessage()).isEqualTo("unreadable");
        assertThat(exception.getCause()).isSameAs(cause);
    }

    @Test
    void the_cause_constructor_accepts_no_cause() {
        LoaderException exception = new LoaderException((Throwable) null);

        assertThat(exception.getMessage()).isNull();
        assertThat(exception.getCause()).isNull();
    }

    @Test
    void the_message_and_cause_constructor_keeps_both() {
        IOException cause = new IOException("unreadable");
        LoaderException exception = new LoaderException("bad value", cause);

        assertThat(exception.getMessage()).isEqualTo("bad value");
        assertThat(exception.getCause()).isSameAs(cause);
    }
}
