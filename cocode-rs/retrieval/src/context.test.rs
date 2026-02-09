use super::*;
use tempfile::TempDir;

#[test]
fn test_features_presets() {
    // NONE
    assert!(!RetrievalFeatures::NONE.code_search);
    assert!(!RetrievalFeatures::NONE.vector_search);
    assert!(!RetrievalFeatures::NONE.query_rewrite);
    assert!(!RetrievalFeatures::NONE.has_search());

    // MINIMAL
    assert!(RetrievalFeatures::MINIMAL.code_search);
    assert!(!RetrievalFeatures::MINIMAL.vector_search);
    assert!(!RetrievalFeatures::MINIMAL.query_rewrite);
    assert!(RetrievalFeatures::MINIMAL.has_search());

    // STANDARD
    assert!(RetrievalFeatures::STANDARD.code_search);
    assert!(!RetrievalFeatures::STANDARD.vector_search);
    assert!(RetrievalFeatures::STANDARD.query_rewrite);
    assert!(RetrievalFeatures::STANDARD.has_search());

    // FULL
    assert!(RetrievalFeatures::FULL.code_search);
    assert!(RetrievalFeatures::FULL.vector_search);
    assert!(RetrievalFeatures::FULL.query_rewrite);
    assert!(RetrievalFeatures::FULL.has_search());
}

#[test]
fn test_features_factory_methods() {
    // none() == NONE
    assert!(!RetrievalFeatures::none().has_search());

    // with_code_search() == MINIMAL
    let features = RetrievalFeatures::with_code_search();
    assert!(features.code_search);
    assert!(!features.vector_search);

    // all() == FULL
    let features = RetrievalFeatures::all();
    assert!(features.code_search);
    assert!(features.vector_search);
    assert!(features.query_rewrite);
}

#[tokio::test]
async fn test_context_creation() {
    let dir = TempDir::new().unwrap();
    let mut config = RetrievalConfig::default();
    config.data_dir = dir.path().to_path_buf();

    let ctx =
        RetrievalContext::new(config, RetrievalFeatures::MINIMAL, dir.path().to_path_buf())
            .await
            .unwrap();

    assert!(ctx.features().code_search);
    assert!(!ctx.features().vector_search);
    assert_eq!(ctx.workspace_root(), dir.path());
}
