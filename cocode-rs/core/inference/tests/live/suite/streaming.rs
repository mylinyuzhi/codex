//! Streaming-specific tests via ApiClient + UnifiedStream::next().

use anyhow::Result;
use cocode_inference::ApiClient;
use cocode_inference::LanguageModel;
use cocode_inference::LanguageModelCallOptions;
use cocode_inference::LanguageModelMessage;
use cocode_inference::QueryResultType;
use cocode_inference::StreamOptions;

/// Verify streaming yields events then Done.
pub async fn run_stream_events(client: &ApiClient, model: &dyn LanguageModel) -> Result<()> {
    let request =
        LanguageModelCallOptions::new(vec![LanguageModelMessage::user_text("Count from 1 to 3.")]);

    let mut stream = client
        .stream_request(model, request, StreamOptions::streaming())
        .await?;

    let mut got_assistant = false;
    let mut got_done = false;

    while let Some(result) = stream.next().await {
        let result = result?;
        match result.result_type {
            QueryResultType::Assistant => got_assistant = true,
            QueryResultType::Done => {
                got_done = true;
                break;
            }
            QueryResultType::Error => {
                anyhow::bail!("Stream error: {:?}", result.error);
            }
            _ => {}
        }
    }

    assert!(
        got_assistant,
        "Should receive at least one Assistant result"
    );
    assert!(got_done, "Should receive Done result");

    Ok(())
}

/// Verify streaming content assembly yields text.
pub async fn run_stream_content(client: &ApiClient, model: &dyn LanguageModel) -> Result<()> {
    let request = LanguageModelCallOptions::new(vec![LanguageModelMessage::user_text(
        "What is the largest planet in our solar system? Reply briefly.",
    )]);

    let mut stream = client
        .stream_request(model, request, StreamOptions::streaming())
        .await?;

    let mut collected_text = String::new();

    while let Some(result) = stream.next().await {
        let result = result?;
        if result.result_type == QueryResultType::Assistant {
            for part in &result.content {
                if let cocode_inference::AssistantContentPart::Text(tp) = part {
                    collected_text.push_str(&tp.text);
                }
            }
        }
        if result.result_type == QueryResultType::Done {
            break;
        }
    }

    assert!(
        !collected_text.is_empty(),
        "Should collect text from stream"
    );
    assert!(
        collected_text.to_lowercase().contains("jupiter"),
        "Should mention Jupiter, got: {collected_text}"
    );

    Ok(())
}

/// Verify event_tx channel receives stream events.
pub async fn run_stream_event_tx(client: &ApiClient, model: &dyn LanguageModel) -> Result<()> {
    let (tx, mut rx) = tokio::sync::mpsc::channel(100);

    let request = LanguageModelCallOptions::new(vec![LanguageModelMessage::user_text("Say hi.")]);

    let stream = client
        .stream_request(model, request, StreamOptions::streaming().with_event_tx(tx))
        .await?;

    // Collect in background
    let collect_handle = tokio::spawn(async move { stream.collect().await });

    // Drain the event channel
    let mut event_count = 0;
    while rx.recv().await.is_some() {
        event_count += 1;
    }

    let _response = collect_handle.await??;

    assert!(
        event_count > 0,
        "Should receive events via event_tx channel"
    );

    Ok(())
}
