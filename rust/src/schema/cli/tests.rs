use std::io;

use super::emit;

#[test]
fn a_rendering_is_written_with_exactly_one_trailing_newline() {
    for rendered in ["body", "body\n", "body\n\n"] {
        let mut out = Vec::new();
        emit(&mut out, rendered).expect("a Vec accepts every write");
        assert_eq!(out, b"body\n", "{rendered:?}");
    }
}

/// A writer standing in for a closed pipe: every write and flush fails, so the write fails first.
struct Closed;

impl io::Write for Closed {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::ErrorKind::BrokenPipe.into())
    }
}

#[test]
fn a_write_failure_is_returned_rather_than_panicking() {
    let error = emit(&mut Closed, "body").expect_err("a closed pipe");
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
}

/// A writer that accepts every byte and then fails to flush them, as a buffered pipe can.
struct FlushFails;

impl io::Write for FlushFails {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::ErrorKind::BrokenPipe.into())
    }
}

#[test]
fn a_flush_failure_is_returned_after_a_successful_write() {
    let error = emit(&mut FlushFails, "body").expect_err("a failed flush");
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
}
