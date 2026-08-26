use std::path::Path;

// The app's version source of truth is the root package.json (bumped by the
// release workflow); the bundled CLI must report that version, not Cargo.toml's.
fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let package_json = Path::new(&manifest_dir).join("../../package.json");
    println!("cargo:rerun-if-changed={}", package_json.display());

    let contents = std::fs::read_to_string(&package_json)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", package_json.display()));
    let parsed: serde_json::Value = serde_json::from_str(&contents)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", package_json.display()));
    let version = parsed["version"]
        .as_str()
        .unwrap_or_else(|| panic!("no version field in {}", package_json.display()));

    println!("cargo:rustc-env=LOOFAH_VERSION={version}");
}
