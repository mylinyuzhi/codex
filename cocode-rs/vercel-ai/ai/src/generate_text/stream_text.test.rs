use std::pin::Pin;
use std::sync::Arc;

use futures::Stream;
use futures::StreamExt;
use vercel_ai_provider::AISdkError;
use vercel_ai_provider::LanguageModelV4StreamPart;
use vercel_ai_provider::LanguageModelV4StreamResult;

use super::*;
use crate::model::LanguageModel;
use crate::prompt::Prompt;
use crate::test_utils::MockLanguageModel;

#[tokio::test]
async fn test_stream_text_no_double_error_on_provider_error() {
    // Verify that a provider stream error only produces one Error event, not two.
    // This mirrors test_stream_object_no_double_error_on_provider_error.
    let model = Arc::new(
        MockLanguageModel::builder()
            .with_stream_handler(|_| {
                let parts: Vec<Result<LanguageModelV4StreamPart, AISdkError>> =
                    vec![Err(AISdkError::new("provider failure"))];
                let stream: Pin<
                    Box<dyn Stream<Item = Result<LanguageModelV4StreamPart, AISdkError>> + Send>,
                > = Box::pin(futures::stream::iter(parts));

                Ok(LanguageModelV4StreamResult {
                    stream,
                    request: None,
                    response: None,
                })
            })
            .build(),
    );

    let options = StreamTextOptions::new(LanguageModel::from_v4(model), Prompt::user("Hello"));

    let result = stream_text(options);
    let mut error_count = 0;
    let mut stream = result.stream;
    while let Some(part) = stream.next().await {
        if matches!(part, TextStreamPart::Error { .. }) {
            error_count += 1;
        }
    }
    // Should only see exactly one error (not two)
    assert_eq!(error_count, 1);
}
