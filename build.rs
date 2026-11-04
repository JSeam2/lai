use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=vendor/sqlite-vector/src");
    println!("cargo:rerun-if-changed=vendor/sqlite-vector/libs");
    let mut vector = cc::Build::new();
    vector
        .include("vendor/sqlite-vector/src")
        .include("vendor/sqlite-vector/libs")
        .define("SQLITE_CORE", None)
        .warnings(false);
    for file in [
        "sqlite-vector.c",
        "distance-cpu.c",
        "distance-neon.c",
        "distance-sse2.c",
        "distance-avx2.c",
        "distance-avx512.c",
        "distance-rvv.c",
    ] {
        vector.file(format!("vendor/sqlite-vector/src/{file}"));
    }
    vector.compile("sqlite_vector");
    println!("cargo:rerun-if-changed=src/pet/window.m");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let object = out.join("pet.o");
    let library = out.join("liblai_pet.a");
    let status = Command::new("clang")
        .args(["-fobjc-arc", "-c", "src/pet/window.m", "-o"])
        .arg(&object)
        .status()
        .expect("clang is required to build the macOS pet");
    assert!(status.success(), "could not compile macOS pet");
    let status = Command::new("ar")
        .arg("crus")
        .arg(&library)
        .arg(&object)
        .status()
        .expect("ar is required to build the macOS pet");
    assert!(status.success(), "could not archive macOS pet");
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=lai_pet");
    for framework in ["AppKit", "WebKit", "Foundation"] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
}
