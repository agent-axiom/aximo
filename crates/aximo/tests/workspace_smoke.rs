#[test]
fn workspace_exposes_expected_crates() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root");
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();

    assert!(manifest.contains("crates/aximo"));
    assert!(manifest.contains("crates/aximo-core"));
    assert!(manifest.contains("crates/aximo-audio"));
    assert!(manifest.contains("crates/aximo-inference"));
}
