use coco_types::JsonRpcMessage;
use coco_types::JsonRpcNotification;
use coco_types::JsonRpcRequest;
use coco_types::JsonRpcResponse;
use coco_types::RequestId;
use pretty_assertions::assert_eq;

use super::*;

fn make_request(id: i64, method: &str) -> JsonRpcMessage {
    JsonRpcMessage::Request(JsonRpcRequest {
        request_id: RequestId::Integer(id),
        method: method.into(),
        params: serde_json::json!({}),
    })
}

fn make_response(id: i64) -> JsonRpcMessage {
    JsonRpcMessage::Response(JsonRpcResponse {
        request_id: RequestId::Integer(id),
        result: serde_json::json!({ "ok": true }),
    })
}

fn make_notification(method: &str) -> JsonRpcMessage {
    JsonRpcMessage::Notification(JsonRpcNotification {
        method: method.into(),
        params: serde_json::json!({}),
    })
}

// ----- InMemoryTransport basic tests ----------------------------------

#[tokio::test]
async fn in_memory_pair_client_to_server() {
    let (server, client) = InMemoryTransport::pair(8);

    // Client sends a request; server reads it.
    let req = make_request(1, "turn/start");
    client.send(req.clone()).await.expect("send");

    let got = server
        .recv()
        .await
        .expect("recv result")
        .expect("message present");
    match (&got, &req) {
        (JsonRpcMessage::Request(a), JsonRpcMessage::Request(b)) => {
            assert_eq!(a.request_id, b.request_id);
            assert_eq!(a.method, b.method);
        }
        _ => panic!("variant mismatch"),
    }
}

#[tokio::test]
async fn in_memory_pair_server_to_client() {
    let (server, client) = InMemoryTransport::pair(8);

    // Server writes a response; client reads it.
    server.send(make_response(42)).await.expect("send");

    let got = client.recv().await.expect("recv").expect("message");
    match got {
        JsonRpcMessage::Response(r) => {
            assert_eq!(r.request_id, RequestId::Integer(42));
            assert_eq!(r.result["ok"], true);
        }
        _ => panic!("expected Response variant"),
    }
}

#[tokio::test]
async fn in_memory_pair_notification_stream() {
    // Server pushes multiple notifications; client receives them in order.
    let (server, client) = InMemoryTransport::pair(16);

    server
        .send(make_notification("turn/started"))
        .await
        .unwrap();
    server
        .send(make_notification("agentMessage/delta"))
        .await
        .unwrap();
    server
        .send(make_notification("turn/completed"))
        .await
        .unwrap();

    let n1 = client.recv().await.unwrap().unwrap();
    let n2 = client.recv().await.unwrap().unwrap();
    let n3 = client.recv().await.unwrap().unwrap();

    let methods: Vec<String> = [n1, n2, n3]
        .iter()
        .filter_map(|m| match m {
            JsonRpcMessage::Notification(n) => Some(n.method.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        methods,
        vec![
            "turn/started".to_string(),
            "agentMessage/delta".to_string(),
            "turn/completed".to_string(),
        ]
    );
}

#[tokio::test]
async fn in_memory_close_blocks_send() {
    let (server, _client) = InMemoryTransport::pair(8);
    server.close().await.unwrap();

    let res = server.send(make_response(1)).await;
    assert!(matches!(res, Err(TransportError::Closed)));
    assert!(!server.is_open());
}

#[tokio::test]
async fn in_memory_recv_returns_none_on_peer_drop() {
    let (server, client) = InMemoryTransport::pair(8);
    // Drop the client. Server's inbox sender (the `a_tx` wrapped in client.outbox)
    // is dropped, so recv() should return Ok(None).
    drop(client);
    let res = server.recv().await.unwrap();
    assert!(res.is_none(), "expected clean EOF, got {res:?}");
}

#[tokio::test]
async fn in_memory_concurrent_send_recv() {
    // Spawn two tasks: one pushing requests, one receiving responses.
    // Verifies thread-safety of Arc<InMemoryTransport>.
    let (server, client) = InMemoryTransport::pair(64);

    let pusher = {
        let client = client.clone();
        tokio::spawn(async move {
            for i in 0..10 {
                client.send(make_request(i, "turn/start")).await.unwrap();
            }
        })
    };

    let receiver = {
        let server = server.clone();
        tokio::spawn(async move {
            let mut got = Vec::new();
            for _ in 0..10 {
                let msg = server.recv().await.unwrap().unwrap();
                got.push(msg);
            }
            got
        })
    };

    pusher.await.unwrap();
    let got = receiver.await.unwrap();
    assert_eq!(got.len(), 10);
    for (i, msg) in got.iter().enumerate() {
        match msg {
            JsonRpcMessage::Request(r) => {
                assert_eq!(r.request_id, RequestId::Integer(i as i64));
            }
            _ => panic!("expected Request"),
        }
    }
}

#[tokio::test]
async fn in_memory_is_open_initially_true() {
    let (server, client) = InMemoryTransport::pair(4);
    assert!(server.is_open());
    assert!(client.is_open());
}

#[tokio::test]
async fn send_roundtrips_all_message_variants() {
    let (server, client) = InMemoryTransport::pair(16);

    // Request
    client.send(make_request(1, "initialize")).await.unwrap();
    // Response
    server.send(make_response(1)).await.unwrap();
    // Notification
    server
        .send(make_notification("session/started"))
        .await
        .unwrap();
    // Error
    server
        .send(JsonRpcMessage::Error(coco_types::JsonRpcError {
            request_id: RequestId::Integer(2),
            code: coco_types::error_codes::METHOD_NOT_FOUND,
            message: "unknown".into(),
            data: None,
        }))
        .await
        .unwrap();

    // Server sees the request
    let req = server.recv().await.unwrap().unwrap();
    assert!(matches!(req, JsonRpcMessage::Request(_)));

    // Client sees response + notification + error
    let resp = client.recv().await.unwrap().unwrap();
    assert!(matches!(resp, JsonRpcMessage::Response(_)));
    let notif = client.recv().await.unwrap().unwrap();
    assert!(matches!(notif, JsonRpcMessage::Notification(_)));
    let err = client.recv().await.unwrap().unwrap();
    assert!(matches!(err, JsonRpcMessage::Error(_)));
}
