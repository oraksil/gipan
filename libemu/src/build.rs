fn main() {
    const MAME_HOME: &str = "../../mame";

    println!("cargo:rustc-link-lib=dylib=mame64d");
    println!("cargo:rustc-link-search=native={}", MAME_HOME);
 }