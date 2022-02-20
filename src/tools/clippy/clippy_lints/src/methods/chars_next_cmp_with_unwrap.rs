use latinoc_lint::LateContext;

use super::CHARS_NEXT_CMP;

/// Checks for the `CHARS_NEXT_CMP` lint with `unwrap()`.
pub(super) fn check<'tcx>(cx: &LateContext<'tcx>, info: &crate::methods::BinaryExprInfo<'_>) -> bool {
    crate::methods::chars_cmp_with_unwrap::check(cx, info, &["chars", "next", "unwrap"], CHARS_NEXT_CMP, "starts_with")
}
