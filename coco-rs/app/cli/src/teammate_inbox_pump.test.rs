//! Tests for the cross-process teammate inbox pump's serialization
//! handshake (`inject_and_wait`) — the novel concurrency logic. Mailbox
//! priority/filtering is covered in `coordinator::runner_loop`
//! (`select_mailbox_prompt`); framing in `coordinator::teammate`.

use super::*;

const FRAMED: &str = "<teammate_message teammate_id=\"team-lead\">\nhi\n</teammate_message>";

fn extract_user_message_id(cmd: UserCommand) -> String {
    match cmd {
        UserCommand::SubmitInput {
            user_message_id,
            content,
            ..
        } => {
            assert!(
                content.contains("teammate_message"),
                "pump must inject framed content, got: {content}"
            );
            user_message_id
        }
        other => panic!("expected SubmitInput, got {other:?}"),
    }
}

#[tokio::test]
async fn inject_and_wait_releases_only_on_own_turn_id() {
    let (command_tx, mut command_rx) = mpsc::channel::<UserCommand>(8);
    let (turn_done_tx, turn_done_rx) = mpsc::channel::<String>(8);
    let cancel = CancellationToken::new();
    let cancel_task = cancel.clone();

    let handle = tokio::spawn(async move {
        let mut rx = turn_done_rx;
        inject_and_wait(&command_tx, &mut rx, &cancel_task, FRAMED.to_string()).await
    });

    let id = extract_user_message_id(command_rx.recv().await.unwrap());

    // A foreign turn completing (human typing in the pane / a drained slash
    // turn) must NOT release the pump.
    turn_done_tx
        .send("some-other-turn".to_string())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(
        !handle.is_finished(),
        "pump released on a foreign turn id — would let drain_active_turn cancel its live turn"
    );

    // Its own turn id releases it.
    turn_done_tx.send(id).await.unwrap();
    assert_eq!(handle.await.unwrap(), Some(()));
}

#[tokio::test]
async fn inject_and_wait_exits_on_cancel() {
    let (command_tx, mut command_rx) = mpsc::channel::<UserCommand>(8);
    let (_turn_done_tx, turn_done_rx) = mpsc::channel::<String>(8);
    let cancel = CancellationToken::new();
    let cancel_task = cancel.clone();

    let handle = tokio::spawn(async move {
        let mut rx = turn_done_rx;
        inject_and_wait(&command_tx, &mut rx, &cancel_task, FRAMED.to_string()).await
    });

    // Drain the injected command so the send succeeds, then cancel while the
    // pump is blocked waiting for completion.
    let _ = extract_user_message_id(command_rx.recv().await.unwrap());
    cancel.cancel();
    assert_eq!(handle.await.unwrap(), None);
}

#[tokio::test]
async fn inject_and_wait_exits_when_handshake_sender_dropped() {
    let (command_tx, mut command_rx) = mpsc::channel::<UserCommand>(8);
    let (turn_done_tx, turn_done_rx) = mpsc::channel::<String>(8);
    let cancel = CancellationToken::new();
    let cancel_task = cancel.clone();

    let handle = tokio::spawn(async move {
        let mut rx = turn_done_rx;
        inject_and_wait(&command_tx, &mut rx, &cancel_task, FRAMED.to_string()).await
    });

    let _ = extract_user_message_id(command_rx.recv().await.unwrap());
    // Driver gone: its handshake sender drops → recv() yields None → exit.
    drop(turn_done_tx);
    assert_eq!(handle.await.unwrap(), None);
}

#[tokio::test]
async fn inject_and_wait_exits_when_command_receiver_dropped() {
    let (command_tx, command_rx) = mpsc::channel::<UserCommand>(8);
    let (_turn_done_tx, mut turn_done_rx) = mpsc::channel::<String>(8);
    let cancel = CancellationToken::new();

    // Driver gone before the inject: the send fails immediately.
    drop(command_rx);
    let result = inject_and_wait(&command_tx, &mut turn_done_rx, &cancel, FRAMED.to_string()).await;
    assert_eq!(result, None);
}
