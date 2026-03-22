use super::*;

#[test]
fn test_noop_telemetry() {
    let telemetry = NoopTelemetry;
    telemetry.on_request(1, Some(StatusCode::OK), None, Duration::from_millis(100));
    telemetry.on_stream_poll(None, Duration::from_micros(10));
    telemetry.on_stream_complete(10, Duration::from_secs(1));
}

#[test]
fn test_logging_telemetry() {
    let telemetry = LoggingTelemetry;

    // Test successful request
    telemetry.on_request(1, Some(StatusCode::OK), None, Duration::from_millis(100));

    // Test failed request
    let error = HyperError::NetworkError("connection refused".to_string());
    telemetry.on_request(2, None, Some(&error), Duration::from_millis(50));

    // Test retry
    telemetry.on_retry(2, Duration::from_secs(1));

    // Test exhausted
    telemetry.on_exhausted(3, &error);

    // Test stream events
    telemetry.on_stream_poll(None, Duration::from_micros(10));
    telemetry.on_stream_complete(100, Duration::from_secs(5));
    telemetry.on_stream_error(&error);
    telemetry.on_idle_timeout(Duration::from_secs(60));
}

#[test]
fn test_telemetry_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<NoopTelemetry>();
    assert_send_sync::<LoggingTelemetry>();
}
