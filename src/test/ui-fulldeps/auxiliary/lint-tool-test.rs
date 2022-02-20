#![feature(rustc_private)]

extern crate latinoc_ast;

// Load rustc as a plugin to get macros
extern crate latinoc_driver;
#[macro_use]
extern crate latinoc_lint;
#[macro_use]
extern crate rustc_session;

use latinoc_driver::plugin::Registry;
use latinoc_lint::{EarlyContext, EarlyLintPass, LintArray, LintContext, LintId, LintPass};
use latinoc_ast as ast;
declare_tool_lint!(pub clippy::TEST_LINT, Warn, "Warn about stuff");
declare_tool_lint!(
    /// Some docs
    pub clippy::TEST_GROUP,
    Warn, "Warn about other stuff"
);

declare_tool_lint!(
    /// Some docs
    pub rustc::TEST_RUSTC_TOOL_LINT,
    Deny,
    "Deny internal stuff"
);

declare_lint_pass!(Pass => [TEST_LINT, TEST_GROUP, TEST_RUSTC_TOOL_LINT]);

impl EarlyLintPass for Pass {
    fn check_item(&mut self, cx: &EarlyContext, it: &ast::Item) {
        if it.ident.name.as_str() == "lintme" {
            cx.lint(TEST_LINT, |lint| {
                lint.build("item is named 'lintme'").set_span(it.span).emit()
            });
        }
        if it.ident.name.as_str() == "lintmetoo" {
            cx.lint(TEST_GROUP, |lint| {
                lint.build("item is named 'lintmetoo'").set_span(it.span).emit()
            });
        }
    }
}

#[no_mangle]
fn __rustc_plugin_registrar(reg: &mut Registry) {
    reg.lint_store.register_lints(&[&TEST_RUSTC_TOOL_LINT, &TEST_LINT, &TEST_GROUP]);
    reg.lint_store.register_early_pass(|| Box::new(Pass));
    reg.lint_store.register_group(
        true,
        "clippy::group",
        Some("clippy_group"),
        vec![LintId::of(&TEST_LINT), LintId::of(&TEST_GROUP)],
    );
}
