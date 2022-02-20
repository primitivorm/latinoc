#[macro_export]
/// Convenience wrapper around rustc's `Symbol::intern`
macro_rules! sym {
    ($tt:tt) => {
        latinoc_span::symbol::Symbol::intern(stringify!($tt))
    };
}
