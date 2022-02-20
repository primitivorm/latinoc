// force-host

#![feature(rustc_private)]

extern crate latinoc_driver;
extern crate rustc_hir;
#[macro_use]
extern crate latinoc_lint;
#[macro_use]
extern crate rustc_session;
extern crate latinoc_ast;
extern crate latinoc_span;

use latinoc_driver::plugin::Registry;
use latinoc_lint::{LateContext, LateLintPass, LintContext};
use latinoc_span::def_id::CRATE_DEF_ID;
use latinoc_span::symbol::Symbol;

declare_lint! {
    CRATE_NOT_OKAY,
    Warn,
    "crate not marked with #![crate_okay]"
}

declare_lint_pass!(Pass => [CRATE_NOT_OKAY]);

impl<'tcx> LateLintPass<'tcx> for Pass {
    fn check_crate(&mut self, cx: &LateContext) {
        let attrs = cx.tcx.hir().attrs(rustc_hir::CRATE_HIR_ID);
        let span = cx.tcx.def_span(CRATE_DEF_ID);
        if !cx.sess().contains_name(attrs, Symbol::intern("crate_okay")) {
            cx.lint(CRATE_NOT_OKAY, |lint| {
                lint.build("crate is not marked with #![crate_okay]").set_span(span).emit()
            });
        }
    }
}

#[no_mangle]
fn __rustc_plugin_registrar(reg: &mut Registry) {
    reg.lint_store.register_lints(&[&CRATE_NOT_OKAY]);
    reg.lint_store.register_late_pass(|| Box::new(Pass));
}
