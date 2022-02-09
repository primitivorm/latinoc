#![crate_type = "dylib"]
#![feature(rustc_private)]

extern crate rustc_middle;
extern crate latinoc_driver;

use latinoc_driver::plugin::Registry;

#[no_mangle]
fn __rustc_plugin_registrar(_: &mut Registry) {}
