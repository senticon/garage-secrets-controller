#[allow(dead_code)]
use super::*;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

#[derive(Clone)]
#[allow(dead_code)]
struct MockResponse {
    status: u16,
    body: String,
}

#[derive(Clone)]
#[allow(dead_code)]
struct RecordedRequest {
    start_line: String,
    headers: String,
    body: String,
}

#[allow(dead_code)]
async fn start_mock_server(
    responses: Vec<MockResponse>,
) -> (String, Arc<Mutex<Vec<RecordedRequest>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let recorded = Arc::new(Mutex::new(Vec::<RecordedRequest>::new()));
    let recorded_clone = Arc::clone(&recorded);

    tokio::spawn(async move {
        for resp in responses {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut bytes = Vec::new();
            let mut tmp = [0_u8; 1024];

            loop {
                let n = socket.read(&mut tmp).await.expect("read");
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&tmp[..n]);
                if bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }

            let header_end = bytes
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map(|i| i + 4)
                .expect("header end");

            let mut request_text = String::from_utf8_lossy(&bytes).to_string();
            let lower_headers = request_text.to_lowercase();
            let content_length = lower_headers
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length: ")
                        .and_then(|value| value.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);

            let body_read = bytes.len().saturating_sub(header_end);
            if content_length > body_read {
                let mut remaining = vec![0_u8; content_length - body_read];
                socket.read_exact(&mut remaining).await.expect("read body");
                bytes.extend_from_slice(&remaining);
                request_text = String::from_utf8_lossy(&bytes).to_string();
            }

            let mut split = request_text.splitn(2, "\r\n\r\n");
            let head = split.next().unwrap_or_default();
            let body = split.next().unwrap_or_default().to_string();
            let mut head_lines = head.lines();
            let start_line = head_lines.next().unwrap_or_default().to_string();
            let headers = head_lines.collect::<Vec<_>>().join("\n");
            recorded_clone.lock().await.push(RecordedRequest {
                start_line,
                headers,
                body,
            });

            let reason = if resp.status == 200 { "OK" } else { "ERR" };
            let response = format!(
                "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",
                resp.status,
                reason,
                resp.body.len(),
                resp.body
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("write response");
        }
    });

    (format!("http://{}", addr), recorded)
}

#[test]
fn parse_json_value_handles_success_and_error() {
    assert!(parse_json_value("/v2/test", r#"{"ok":true}"#).is_ok());
    assert!(parse_json_value("/v2/test", "not-json").is_err());
}

#[test]
fn helpers_parse_bucket_and_key_shapes() {
    let obj = serde_json::json!({"id": "b1"});
    assert_eq!(bucket_from_value(&obj).expect("bucket").id, "b1");

    let key_json = serde_json::json!({"accessKeyId": "k1", "secretAccessKey": "s1", "name": "n1"});
    let key = key_from_value(&key_json).expect("key");
    assert_eq!(key.access_key_id, "k1");
    assert_eq!(key.secret_access_key.as_deref(), Some("s1"));
    assert_eq!(key.name.as_deref(), Some("n1"));

    assert!(bucket_from_value(&serde_json::json!(null)).is_none());
    assert!(key_from_value(&serde_json::json!({"name": "missing-id"})).is_none());
}

#[tokio::test]
async fn get_status_success_and_error() {
    let (base_ok, _) = start_mock_server(vec![MockResponse {
        status: 200,
        body: "{\"status\":\"ok\"}".to_string(),
    }])
    .await;
    let client_ok = GarageClient::new(base_ok, "token".to_string());
    let status = client_ok.get_status().await.expect("status ok");
    assert_eq!(status["status"], "ok");

    let (base_err, _) = start_mock_server(vec![MockResponse {
        status: 500,
        body: "boom".to_string(),
    }])
    .await;
    let client_err = GarageClient::new(base_err, "token".to_string());
    match client_err.get_status().await.expect_err("status err") {
        AppError::GarageApi { status, message } => {
            assert_eq!(status, 500);
            assert_eq!(message, "boom");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn create_bucket_covers_direct_and_fallback_paths() {
    let (base_direct, _) = start_mock_server(vec![MockResponse {
        status: 200,
        body: "{\"id\":\"bucket-direct\"}".to_string(),
    }])
    .await;
    let client_direct = GarageClient::new(base_direct, "token".to_string());
    let direct = client_direct
        .create_bucket("alpha")
        .await
        .expect("direct bucket");
    assert_eq!(direct.id, "bucket-direct");

    let (base_fallback, _) = start_mock_server(vec![MockResponse {
        status: 200,
        body: "{\"id\":\"bucket-fallback\"}".to_string(),
    }])
    .await;
    let client_fallback = GarageClient::new(base_fallback, "token".to_string());
    let fallback = client_fallback
        .create_bucket("beta")
        .await
        .expect("fallback bucket");
    assert_eq!(fallback.id, "bucket-fallback");

    let (base_missing, _) = start_mock_server(vec![
        MockResponse {
            status: 200,
            body: "{}".to_string(),
        },
        MockResponse {
            status: 404,
            body: "not found".to_string(),
        },
    ])
    .await;
    let client_missing = GarageClient::new(base_missing, "token".to_string());
    let missing = client_missing.create_bucket("gamma").await;
    assert!(
        matches!(missing, Err(AppError::Resource(msg)) if msg.contains("lookup by alias failed"))
    );
}

#[tokio::test]
async fn create_key_lookup_and_allow_bucket_key() {
    let (base_direct, _) = start_mock_server(vec![MockResponse {
        status: 200,
        body: "{\"accessKeyId\":\"k-direct\",\"name\":\"n\"}".to_string(),
    }])
    .await;
    let client_direct = GarageClient::new(base_direct, "token".to_string());
    let direct = client_direct.create_key("n").await.expect("direct key");
    assert_eq!(direct.access_key_id, "k-direct");

    let (base_none, _) = start_mock_server(vec![
        MockResponse {
            status: 200,
            body: "{}".to_string(),
        },
        MockResponse {
            status: 404,
            body: "not found".to_string(),
        },
    ])
    .await;
    let client_none = GarageClient::new(base_none, "token".to_string());
    let none = client_none.create_key("ghost").await;
    assert!(matches!(none, Err(AppError::Resource(msg)) if msg.contains("lookup failed")));

    let (base_allow, requests) = start_mock_server(vec![MockResponse {
        status: 200,
        body: "{}".to_string(),
    }])
    .await;
    let client_allow = GarageClient::new(base_allow, "token-123".to_string());
    client_allow
        .allow_bucket_key("bucket-x", "key-x", true, false, true)
        .await
        .expect("allow ok");
    let reqs = requests.lock().await;
    let req = reqs.first().expect("recorded request");
    assert!(req.start_line.contains("POST /v2/AllowBucketKey HTTP/1.1"));
    assert!(req
        .headers
        .to_lowercase()
        .contains("authorization: bearer token-123"));
    assert!(req.body.contains("\"bucketId\":\"bucket-x\""));
    assert!(req.body.contains("\"accessKeyId\":\"key-x\""));
    assert!(req.body.contains("\"read\":true"));
    assert!(req.body.contains("\"write\":false"));
    assert!(req.body.contains("\"owner\":true"));
}
