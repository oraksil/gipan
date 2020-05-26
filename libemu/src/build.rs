use std::path::PathBuf;

extern crate bindgen;

fn main() {
    let bindings = bindgen::Builder::default()
        .header("include/headless.h")
        .clang_arg("-xc++")
        .clang_arg("-std=c++14")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .generate()
        .expect("Unable to generate bindings");

    bindings
        .write_to_file(PathBuf::from("src/bindings.rs"))
        .expect("Couldn't write bindings!");

    const MAME_HOME: &str = "../mame";
    println!("cargo:rustc-link-lib=dylib=mame64");
    println!("cargo:rustc-link-search=native={}", MAME_HOME);
 }