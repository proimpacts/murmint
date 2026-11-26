fn main() {
    // extra tests that only exist on the maintainer's PC
    println!("cargo:rustc-check-cfg=cfg(local_tests)");
    println!("cargo:rerun-if-changed=src/session_tests.rs");
    if std::path::Path::new("src/session_tests.rs").exists() {
        println!("cargo:rustc-cfg=local_tests");
    }
}
