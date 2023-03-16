use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-env=BUILD_TRIPLE={}", env::var("HOST").unwrap());

    // This may not be a canonicalized path.
    // TODO: proman. To debug bootstrap with vscode. let mut rustc = PathBuf::from(env::var_os("RUSTC").unwrap());
    let mut rustc = PathBuf::from(env::var_os("RUSTC").unwrap());
    // let mut rustc = PathBuf::from(
    //     "C:\\Users\\ciber\\.rustup\\toolchains\\1.58.1-x86_64-pc-windows-msvc\\bin\\rustc.exe",
    // );
    println!(">>> RUSTC: {}", rustc.to_str().unwrap());

    if rustc.is_relative() {
        for dir in env::split_paths(&env::var_os("PATH").unwrap_or_default()) {
            let absolute = dir.join(&rustc);
            if absolute.exists() {
                rustc = absolute;
                break;
            }
        }
    }
    assert!(rustc.is_absolute());

    // FIXME: if the path is not utf-8, this is going to break. Unfortunately
    // Cargo doesn't have a way for us to specify non-utf-8 paths easily, so
    // we'll need to invent some encoding scheme if this becomes a problem.
    println!("cargo:rustc-env=RUSTC={}", rustc.to_str().unwrap());
    println!(">>> RUSTC: {}", rustc.to_str().unwrap());
}
