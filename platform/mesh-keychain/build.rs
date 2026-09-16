//! Build the macOS Secure Enclave bridge only for the platform that owns those APIs.

fn main() {
    println!("cargo:rerun-if-changed=src/secure_enclave.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("src/secure_enclave.m")
            .flag("-fobjc-arc")
            .compile("mesh_secure_enclave");
        println!("cargo:rustc-link-lib=framework=Security");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=LocalAuthentication");
    }
}
