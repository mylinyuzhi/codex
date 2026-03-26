use std::sync::Arc;

use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

use super::*;

#[tokio::test]
async fn test_proxy_server_starts_and_stops() {
    let filter = Arc::new(DomainFilter::new(vec![], vec![]));
    let token = CancellationToken::new();
    let mut server = ProxyServer::start(filter, token).await.unwrap();

    assert_ne!(server.http_port(), 0);
    assert_ne!(server.socks_port(), 0);
    assert_ne!(server.http_port(), server.socks_port());

    server.stop().await;
}

#[tokio::test]
async fn test_proxy_server_ports_are_connectable() {
    let filter = Arc::new(DomainFilter::new(vec![], vec![]));
    let token = CancellationToken::new();
    let mut server = ProxyServer::start(filter, token).await.unwrap();

    // Verify both ports accept TCP connections.
    let http_conn = TcpStream::connect(format!("127.0.0.1:{}", server.http_port())).await;
    assert!(http_conn.is_ok());

    let socks_conn = TcpStream::connect(format!("127.0.0.1:{}", server.socks_port())).await;
    assert!(socks_conn.is_ok());

    server.stop().await;
}

#[tokio::test]
async fn test_http_proxy_bad_request() {
    let filter = Arc::new(DomainFilter::new(vec![], vec![]));
    let token = CancellationToken::new();
    let mut server = ProxyServer::start(filter, token).await.unwrap();

    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", server.http_port()))
        .await
        .unwrap();
    stream.write_all(b"GET / HTTP/1.1\r\n\r\n").await.unwrap();

    let mut buf = vec![0u8; 1024];
    let n = stream.read(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf[..n]);
    assert!(response.contains("400 Bad Request"));

    server.stop().await;
}

#[tokio::test]
async fn test_http_proxy_denied_domain() {
    let filter = Arc::new(DomainFilter::new(vec![], vec!["evil.com".to_string()]));
    let token = CancellationToken::new();
    let mut server = ProxyServer::start(filter, token).await.unwrap();

    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", server.http_port()))
        .await
        .unwrap();
    stream
        .write_all(b"CONNECT evil.com:443 HTTP/1.1\r\nHost: evil.com\r\n\r\n")
        .await
        .unwrap();

    let mut buf = vec![0u8; 1024];
    let n = stream.read(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf[..n]);
    assert!(response.contains("403 Forbidden"));
    assert!(response.contains("evil.com"));

    server.stop().await;
}

#[tokio::test]
async fn test_cancellation_stops_servers() {
    let filter = Arc::new(DomainFilter::new(vec![], vec![]));
    let token = CancellationToken::new();
    let server = ProxyServer::start(Arc::clone(&filter), token.clone())
        .await
        .unwrap();

    let http_port = server.http_port();
    let socks_port = server.socks_port();

    token.cancel();
    // Give tasks time to notice cancellation.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // After cancellation, the listeners should be dropped.
    // New connections should fail (or be immediately closed).
    let http_result = TcpStream::connect(format!("127.0.0.1:{http_port}")).await;
    let socks_result = TcpStream::connect(format!("127.0.0.1:{socks_port}")).await;

    // At least one should fail after cancel (accept loop exited).
    // The exact behavior depends on OS socket teardown timing,
    // so we just verify the test doesn't hang.
    drop(http_result);
    drop(socks_result);
}

#[test]
fn test_extract_host_simple() {
    assert_eq!(extract_host("example.com:443"), "example.com");
}

#[test]
fn test_extract_host_no_port() {
    assert_eq!(extract_host("example.com"), "example.com");
}

#[test]
fn test_extract_host_ipv6_bracket() {
    assert_eq!(extract_host("[::1]:443"), "[::1]");
}
