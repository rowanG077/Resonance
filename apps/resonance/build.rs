use std::process::Command;

fn output(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|result| result.status.success())
        .map(|result| String::from_utf8_lossy(&result.stdout).trim().to_owned())
        .filter(|result| !result.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=../../crates");
    println!("cargo:rerun-if-changed=../../Cargo.toml");
    println!("cargo:rerun-if-changed=../../Cargo.lock");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/index");
    let reference = output("git", &["symbolic-ref", "-q", "HEAD"]);
    if reference.starts_with("refs/") {
        println!("cargo:rerun-if-changed=../../.git/{reference}");
    }
    for (name, value) in [
        ("RESONANCE_REVISION", output("git", &["rev-parse", "HEAD"])),
        (
            "RESONANCE_SOURCE",
            output("git", &["describe", "--always", "--dirty", "--abbrev=40"]),
        ),
        ("RESONANCE_RUSTC", output("rustc", &["--version"])),
        ("RESONANCE_TARGET", std::env::var("TARGET").unwrap()),
        ("RESONANCE_PROFILE", std::env::var("PROFILE").unwrap()),
    ] {
        println!("cargo:rustc-env={name}={value}");
    }
}
