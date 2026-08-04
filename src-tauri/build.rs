fn main() {
    use std::path::PathBuf;
    use std::process::Command;

    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by Cargo"),
    );
    let workspace_dir = manifest_dir
        .parent()
        .expect("src-tauri must be inside the workspace root");
    let output = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR must be set by Cargo"))
        .join("networks.json");

    let status = Command::new("bun")
        .arg("scripts/generate-network-registry.ts")
        .arg(&output)
        .current_dir(workspace_dir)
        .status()
        .unwrap_or_else(|error| {
            panic!("Bun is required to generate the Rust network registry: {error}")
        });
    assert!(status.success(), "network registry generation failed");

    println!("cargo:rerun-if-changed=../src/networks.json");
    println!("cargo:rerun-if-changed=../src/networks.ts");
    println!("cargo:rerun-if-changed=../src/types.ts");
    println!("cargo:rerun-if-changed=../scripts/generate-network-registry.ts");
    tauri_build::build();
}
