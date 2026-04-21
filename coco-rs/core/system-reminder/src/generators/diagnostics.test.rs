use super::*;
use crate::generator::DiagnosticFileSummary;
use crate::generator::GeneratorContext;
use coco_config::SystemReminderConfig;

#[tokio::test]
async fn skips_when_no_diagnostics() {
    let c = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder(&c).diagnostics(vec![]).build();
    assert!(DiagnosticsGenerator.generate(&ctx).await.unwrap().is_none());
}

#[tokio::test]
async fn emits_wrapped_in_new_diagnostics_tag() {
    let c = SystemReminderConfig::default();
    let diags = vec![
        DiagnosticFileSummary {
            path: "foo.rs".into(),
            formatted: "foo.rs: 3 errors".into(),
        },
        DiagnosticFileSummary {
            path: "bar.rs".into(),
            formatted: "bar.rs: 1 warning".into(),
        },
    ];
    let ctx = GeneratorContext::builder(&c).diagnostics(diags).build();
    let text = DiagnosticsGenerator
        .generate(&ctx)
        .await
        .unwrap()
        .unwrap()
        .content()
        .unwrap()
        .to_string();
    assert!(text.starts_with("<new-diagnostics>"));
    assert!(text.ends_with("</new-diagnostics>"));
    assert!(text.contains("The following new diagnostic issues were detected"));
    assert!(text.contains("foo.rs: 3 errors"));
    assert!(text.contains("bar.rs: 1 warning"));
}
