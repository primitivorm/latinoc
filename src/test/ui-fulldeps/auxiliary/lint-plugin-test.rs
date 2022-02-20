// force-host

#![feature(rustc_private)]

extern crate latinoc_ast;

// Load rustc as a plugin to get macros
extern crate latinoc_driver;
#[macro_use]
extern crate latinoc_lint;
#[macro_use]
extern crate rustc_session;

use latinoc_driver::plugin::Registry;
use latinoc_lint::{EarlyContext, EarlyLintPass, LintArray, LintContext, LintPass};
use latinoc_ast as ast;
declare_lint!(TEST_LINT, Warn, "Warn about items named 'lintme'");

declare_lint_pass!(Pass => [TEST_LINT]);

impl EarlyLintPass for Pass {
    fn check_item(&mut self, cx: &EarlyContext, it: &ast::Item) {
        if it.ident.name.as_str() == "lintme" {
            cx.lint(TEST_LINT, |lint| {
                lint.build("item is named 'lintme'").set_span(it.span).emit()
            });
        }
    }
}

#[no_mangle]
fn __rustc_plugin_registrar(reg: &mut Registry) {
    reg.lint_store.register_lints(&[&TEST_LINT]);
    reg.lint_store.register_early_pass(|| Box::new(Pass));
}
