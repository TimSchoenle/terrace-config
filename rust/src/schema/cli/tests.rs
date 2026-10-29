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

/// A writer standing in for a closed pipe: every write and flush fails.
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
