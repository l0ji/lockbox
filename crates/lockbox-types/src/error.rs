//! What the errors of the layers below are reported with.

use std::error::Error;

/// Deepest cause in an error chain, which is the one that names the problem.
pub fn root_cause<'a>(error: &'a (dyn Error + 'static)) -> &'a (dyn Error + 'static) {
    let mut cause = error;
    while let Some(next) = cause.source() {
        cause = next;
    }
    cause
}
