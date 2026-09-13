fn main() {
    println!("cargo:rustc-link-arg-bins=/ENTRY:mainCRTStartup");
    println!("cargo:rustc-cdylib-link-arg=/ENTRY:DllMain");
}
