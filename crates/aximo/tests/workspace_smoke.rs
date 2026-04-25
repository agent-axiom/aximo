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

#[test]
fn workspace_exposes_runtime_setup_artifacts() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root");
    let justfile = std::fs::read_to_string(root.join("justfile")).unwrap();

    assert!(root.join("Dockerfile").exists());
    assert!(root.join("docker-compose.yml").exists());
    assert!(root.join("scripts/fetch-models.sh").exists());
    assert!(justfile.contains("setup-models:"));
}

#[test]
fn workspace_exposes_container_release_workflow() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root");
    let workflow = std::fs::read_to_string(root.join(".github/workflows/container.yml")).unwrap();

    assert!(workflow.contains("ghcr.io/"));
    assert!(workflow.contains("docker/build-push-action"));
    assert!(workflow.contains("type=semver"));
}

#[test]
fn workspace_docs_keep_transcription_query_examples_valid() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root");
    let readme = std::fs::read_to_string(root.join("README.md")).unwrap();
    let architecture = std::fs::read_to_string(root.join("docs/architecture.md")).unwrap();

    assert!(readme.contains("language=ru&timestamps=true"));
    assert!(!readme.contains("×tamps"));
    assert!(!architecture.contains("×tamps"));
}

#[test]
fn workspace_example_config_documents_runtime_degraded_policy() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root");
    let config = std::fs::read_to_string(root.join("config/aximo.example.toml")).unwrap();

    assert!(config.contains("runtime_degraded_policy = \"readiness_only\""));
}
