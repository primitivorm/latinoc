// force-host

#![feature(rustc_private)]

extern crate latinoc_driver;
use latinoc_driver::plugin::Registry;

#[no_mangle]
fn __rustc_plugin_registrar(_: &mut Registry) {}
