use std::path::PathBuf;

use aximo_inference::runtime::{EngineKind, EngineSpec, RuntimeEngineFactory};

#[test]
fn runtime_loader_rejects_missing_model_directory() {
    let factory = RuntimeEngineFactory::default();
    let spec = EngineSpec {
        kind: EngineKind::Parakeet,
        model_path: PathBuf::from("/definitely/missing/model"),
    };

    let error = match factory.build(&spec) {
        Ok(_) => panic!("expected loader to reject a missing model directory"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("does not exist"));
}
