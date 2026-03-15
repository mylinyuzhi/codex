use super::*;
use vercel_ai_provider::TextPart;

#[test]
fn converts_simple_prompt() {
    let prompt = vec![
        LanguageModelV4Message::System {
            content: "Be helpful".into(),
            provider_options: None,
        },
        LanguageModelV4Message::User {
            content: vec![UserContentPart::Text(TextPart {
                text: "Hello".into(),
                provider_metadata: None,
            })],
            provider_options: None,
        },
    ];
    let result = convert_to_completion_prompt(&prompt);
    assert_eq!(result, "Be helpful\n\nHello");
}
