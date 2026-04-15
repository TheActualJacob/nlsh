use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let shim_dir = manifest_dir.parent().unwrap().join("nlsh-model");
    let shim_release = shim_dir.join(".build/release/nlsh-model");

    println!("cargo:rerun-if-changed=../nlsh-model/Sources/nlsh-model/main.swift");
    println!("cargo:rerun-if-changed=../nlsh-model/Package.swift");

    let ok = Command::new("swift")
        .args(["build", "-c", "release"])
        .current_dir(&shim_dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if ok {
        println!(
            "cargo:rustc-env=NLSH_MODEL_BUILD_PATH={}",
            shim_release.display()
        );
    } else {
        // Emit warning; don't hard-fail the Rust build.
        // At runtime, check_available() will return false and NL routing is disabled.
        println!("cargo:warning=nlsh-model Swift shim build failed — NL routing will be disabled");
        println!("cargo:rustc-env=NLSH_MODEL_BUILD_PATH=");
    }
}
