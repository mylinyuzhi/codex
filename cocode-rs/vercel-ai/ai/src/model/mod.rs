//! Model resolution module.

mod resolve_model;

pub use resolve_model::EmbeddingModel;
pub use resolve_model::ImageModelRef;
pub use resolve_model::LanguageModel;
pub use resolve_model::VideoModelRef;
pub use resolve_model::resolve_embedding_model;
pub use resolve_model::resolve_embedding_model_with_provider;
pub use resolve_model::resolve_image_model;
pub use resolve_model::resolve_image_model_with_provider;
pub use resolve_model::resolve_language_model;
pub use resolve_model::resolve_language_model_with_provider;
pub use resolve_model::resolve_video_model;
pub use resolve_model::resolve_video_model_with_provider;
