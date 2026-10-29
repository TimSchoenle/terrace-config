use std::ffi::OsString;

use super::unicode;

#[test]
fn unicode_arguments_pass_through_in_order() {
    let args = ["--format", "contract"].map(OsString::from);
    assert_eq!(
        unicode(args).expect("valid Unicode"),
        ["--format", "contract"]
    );
}

#[cfg(unix)]
#[test]
fn an_argument_that_is_not_unicode_is_refused_rather_than_panicking() {
    use std::os::unix::ffi::OsStringExt as _;

    let args = [
        OsString::from("--only"),
        OsString::from_vec(vec![b'k', 0xFF, b'y']),
    ];
    let error = unicode(args).expect_err("an invalid byte sequence");
    assert!(error.message().contains("is not valid Unicode"), "{error}");
    assert!(error.to_string().ends_with(super::USAGE), "{error}");
}
