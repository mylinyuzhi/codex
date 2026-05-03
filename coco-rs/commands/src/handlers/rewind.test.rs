use super::*;

#[tokio::test]
async fn rewind_opens_message_selector() {
    let h = RewindHandler;
    let r = h.execute_command("").await.unwrap();
    match r {
        CommandResult::OpenDialog(DialogSpec::MessageSelector) => {}
        other => panic!("unexpected: {other:?}"),
    }
}
