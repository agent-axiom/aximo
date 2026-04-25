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
    let benchmark = std::fs::read_to_string(root.join("scripts/benchmark-api.sh")).unwrap();
    let typo = ["×", "tamps"].concat();

    assert!(readme.contains("language=ru&timestamps=true"));
    assert!(benchmark.contains(r#"engine=${engine}&timestamps=${TIMESTAMPS}"#));
    assert!(!readme.contains(&typo));
    assert!(!architecture.contains(&typo));
    assert!(!benchmark.contains(&typo));
}

#[test]
fn workspace_text_artifacts_do_not_contain_timestamp_query_typo() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root");
    let mut files = vec![
        root.join("README.md"),
        root.join("scripts/benchmark-api.sh"),
    ];
    collect_text_files(&root.join("docs"), &mut files);
    collect_text_files(&root.join("crates/aximo/tests"), &mut files);
    let typo = ["×", "tamps"].concat();

    for path in files {
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(
            !contents.contains(&typo),
            "{} contains broken timestamps query spelling",
            path.display()
        );
    }
}

fn collect_text_files(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_text_files(&path, files);
        } else if matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("md" | "rs" | "sh" | "toml" | "yaml" | "yml")
        ) {
            files.push(path);
        }
    }
}

#[test]
fn workspace_example_config_documents_runtime_degraded_policy() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root");
    let config = std::fs::read_to_string(root.join("config/aximo.example.toml")).unwrap();

    assert!(config.contains("runtime_degraded_policy = \"readiness_only\""));
    assert!(config.contains("runtime_degraded_recovery_cooldown_ms = 30000"));
}

#[test]
fn workspace_exposes_benchmark_suite() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root");
    let justfile = std::fs::read_to_string(root.join("justfile")).unwrap();
    let docs = std::fs::read_to_string(root.join("docs/benchmarks.md")).unwrap();
    let baseline_docs = root.join("docs/benchmark-baselines.md");

    assert!(root.join("scripts/benchmark-api.sh").exists());
    assert!(root.join("scripts/render-benchmark-report.sh").exists());
    assert!(root.join("scripts/generate-benchmark-fixtures.sh").exists());
    assert!(baseline_docs.exists());
    assert!(justfile.contains("benchmark-api:"));
    assert!(justfile.contains("benchmark-report:"));
    assert!(justfile.contains("benchmark-fixtures:"));
    assert!(docs.contains("RTF"));
    assert!(docs.contains("WER"));
    assert!(docs.contains("AXIMO_BENCH_FIXTURES_DIR"));
    assert!(docs.contains("benchmark-report.md"));
    assert!(docs.contains("generate-benchmark-fixtures.sh"));
    assert!(docs.contains("benchmark-baselines.md"));
    assert!(docs.contains("Parakeet"));
    assert!(docs.contains("GigaAM"));
}

#[test]
fn workspace_exposes_kubernetes_manifests() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root");
    let deployment =
        std::fs::read_to_string(root.join("deploy/kubernetes/deployment.yaml")).unwrap();
    let docs = std::fs::read_to_string(root.join("docs/kubernetes.md")).unwrap();

    assert!(root.join("deploy/kubernetes/kustomization.yaml").exists());
    assert!(root.join("deploy/kubernetes/service.yaml").exists());
    assert!(root.join("deploy/kubernetes/configmap.yaml").exists());
    assert!(root.join("deploy/kubernetes/pvc.yaml").exists());
    assert!(deployment.contains("readinessProbe"));
    assert!(deployment.contains("livenessProbe"));
    assert!(deployment.contains("AXIMO_RUNTIME_DEGRADED_POLICY"));
    assert!(deployment.contains("AXIMO_RUNTIME_DEGRADED_RECOVERY_COOLDOWN_MS"));
    assert!(docs.contains("kubectl apply -k deploy/kubernetes"));
}

#[test]
fn workspace_exposes_security_release_hardening() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root");
    let security_workflow =
        std::fs::read_to_string(root.join(".github/workflows/security.yml")).unwrap();
    let container_workflow =
        std::fs::read_to_string(root.join(".github/workflows/container.yml")).unwrap();
    let security_policy = std::fs::read_to_string(root.join("SECURITY.md")).unwrap();

    assert!(root.join("deny.toml").exists());
    assert!(security_workflow.contains("cargo audit"));
    assert!(security_workflow.contains("cargo deny check"));
    assert!(security_workflow.contains("cargo cyclonedx"));
    assert!(container_workflow.contains("sbom: true"));
    assert!(container_workflow.contains("provenance: true"));
    assert!(security_policy.contains("Reporting a Vulnerability"));
}
