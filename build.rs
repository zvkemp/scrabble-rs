use std::process::Command;
fn main() {
    Command::new("yarn")
        .args(["build"])
        .output()
        .expect("failed to build js");

    println!("cargo:rerun-if-changed=js");
    println!("cargo:rerun-if-changed=css");
    println!("cargo:rerun-if-changed=package.json");
    println!("cargo:rerun-if-changed=yarn.lock");
    println!("cargo:rerun-if-changed=tsconfig.json");
    println!("cargo:rerun-if-changed=build.rs");
}
