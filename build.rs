use std::process::Command;
fn main() {
    Command::new("yarn")
        .args(["build"])
        .output()
        .expect("failed to build js");

    println!("cargo:rerun-if-changed=js/index.ts");
    println!("cargo:rerun-if-changed=build.rs");
}
