use super::*;
#[allow(unused_imports)]
use serde::Deserialize;
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
    let addr = listener.local_addr().expect("addr");
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
            let content_length = request_text
                .to_lowercase()
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
            let mut lines = head.lines();
            let start_line = lines.next().unwrap_or_default().to_string();
            let headers = lines.collect::<Vec<_>>().join("\n");
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

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Demo {
    name: String,
}

#[test]
fn parse_kv_v2_read_shape() {
    let body = r#"{"data":{"data":{"name":"x"}}}"#;
    let parsed: super::ReadResponse<Demo> = serde_json::from_str(body).expect("parse");
    assert_eq!(parsed.data.data.name, "x");
}

#[test]
fn parse_json_handles_success_and_error() {
    let ok: ListResponse = parse_json("metadata", "p", r#"{"data":{"keys":["a/"]}}"#).expect("ok");
    assert_eq!(ok.data.keys, vec!["a/"]);
    assert!(parse_json::<ListResponse>("metadata", "p", "nope").is_err());
}

#[tokio::test]
async fn list_covers_success_not_found_and_error() {
    let (base_ok, reqs_ok) = start_mock_server(vec![MockResponse {
        status: 200,
        body: r#"{"data":{"keys":["a/","b/"]}}"#.to_string(),
    }])
    .await;
    let client_ok = OpenBaoClient::new(
        base_ok,
        "secret".to_string(),
        "".to_string(),
        "token-1".to_string(),
    );
    let keys = client_ok.list("apps").await.expect("list ok");
    assert_eq!(keys, vec!["a/", "b/"]);
    let reqs = reqs_ok.lock().await;
    let req = reqs.first().expect("request");
    assert!(req
        .start_line
        .contains("LIST /v1/secret/metadata/apps HTTP/1.1"));
    assert!(req
        .headers
        .to_lowercase()
        .contains("x-vault-token: token-1"));

    let (base_nf, _) = start_mock_server(vec![MockResponse {
        status: 404,
        body: "missing".to_string(),
    }])
    .await;
    let client_nf = OpenBaoClient::new(
        base_nf,
        "secret".to_string(),
        "".to_string(),
        "token-2".to_string(),
    );
    let empty = client_nf.list("apps").await.expect("not found handled");
    assert!(empty.is_empty());

    let (base_err, _) = start_mock_server(vec![MockResponse {
        status: 500,
        body: "boom".to_string(),
    }])
    .await;
    let client_err = OpenBaoClient::new(
        base_err,
        "secret".to_string(),
        "".to_string(),
        "token-3".to_string(),
    );
    match client_err.list("apps").await.expect_err("list err") {
        AppError::OpenBaoApi { status, message } => {
            assert_eq!(status, 500);
            assert_eq!(message, "boom");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn read_secret_covers_success_not_found_and_error() {
    let (base_ok, reqs_ok) = start_mock_server(vec![MockResponse {
        status: 200,
        body: r#"{"data":{"data":{"name":"alice"}}}"#.to_string(),
    }])
    .await;
    let client_ok = OpenBaoClient::new(
        base_ok,
        "secret".to_string(),
        "".to_string(),
        "token-a".to_string(),
    );
    let found: Option<Demo> = client_ok.read_secret("users/alice").await.expect("read ok");
    assert_eq!(found.expect("value").name, "alice");
    let reqs = reqs_ok.lock().await;
    let req = reqs.first().expect("request");
    assert!(req
        .start_line
        .contains("GET /v1/secret/data/users/alice HTTP/1.1"));

    let (base_nf, _) = start_mock_server(vec![MockResponse {
        status: 404,
        body: "missing".to_string(),
    }])
    .await;
    let client_nf = OpenBaoClient::new(
        base_nf,
        "secret".to_string(),
        "".to_string(),
        "token-b".to_string(),
    );
    let none: Option<Demo> = client_nf.read_secret("users/missing").await.expect("none");
    assert!(none.is_none());

    let (base_err, _) = start_mock_server(vec![MockResponse {
        status: 403,
        body: "denied".to_string(),
    }])
    .await;
    let client_err = OpenBaoClient::new(
        base_err,
        "secret".to_string(),
        "".to_string(),
        "token-c".to_string(),
    );
    match client_err
        .read_secret::<Demo>("users/alice")
        .await
        .expect_err("read err")
    {
        AppError::OpenBaoApi { status, message } => {
            assert_eq!(status, 403);
            assert_eq!(message, "denied");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn write_secret_covers_success_and_error() {
    let (base_ok, reqs_ok) = start_mock_server(vec![MockResponse {
        status: 200,
        body: "{}".to_string(),
    }])
    .await;
    let client_ok = OpenBaoClient::new(
        base_ok,
        "secret".to_string(),
        "".to_string(),
        "token-z".to_string(),
    );
    let payload = serde_json::json!({"access":"granted"});
    client_ok
        .write_secret("apps/demo", &payload)
        .await
        .expect("write ok");
    let reqs = reqs_ok.lock().await;
    let req = reqs.first().expect("request");
    assert!(req
        .start_line
        .contains("POST /v1/secret/data/apps/demo HTTP/1.1"));
    assert!(req
        .headers
        .to_lowercase()
        .contains("x-vault-token: token-z"));
    assert!(req.body.contains("\"data\":{\"access\":\"granted\"}"));

    let (base_err, _) = start_mock_server(vec![MockResponse {
        status: 400,
        body: "bad request".to_string(),
    }])
    .await;
    let client_err = OpenBaoClient::new(
        base_err,
        "secret".to_string(),
        "ns-err".to_string(),
        "token-y".to_string(),
    );
    let err = client_err.write_secret("apps/demo", &payload).await;
    assert!(matches!(err, Err(AppError::OpenBaoApi { status: 400, .. })));
}

#[tokio::test]
async fn list_sends_namespace_header_when_set() {
    let (base, reqs) = start_mock_server(vec![MockResponse {
        status: 200,
        body: r#"{"data":{"keys":["a/"]}}"#.to_string(),
    }])
    .await;
    let client = OpenBaoClient::new(
        base,
        "secret".to_string(),
        "custo".to_string(),
        "t".to_string(),
    );
    client.list("p").await.expect("ok");
    let reqs = reqs.lock().await;
    let req = reqs.first().expect("req");
    assert!(req
        .headers
        .to_lowercase()
        .contains("x-vault-namespace: custo"));
    assert!(req.headers.to_lowercase().contains("x-vault-token: t"));
}

#[tokio::test]
async fn read_sends_namespace_header_when_set() {
    let (base, reqs) = start_mock_server(vec![MockResponse {
        status: 200,
        body: r#"{"data":{"data":{}}}"#.to_string(),
    }])
    .await;
    let client = OpenBaoClient::new(
        base,
        "secret".to_string(),
        "the_ns".to_string(),
        "tk".to_string(),
    );
    client.read_secret::<Value>("p").await.expect("ok");
    let reqs = reqs.lock().await;
    let req = reqs.first().expect("req");
    assert!(req
        .headers
        .to_lowercase()
        .contains("x-vault-namespace: the_ns"));
}

#[tokio::test]
async fn write_sends_namespace_header_when_set() {
    let (base, reqs) = start_mock_server(vec![MockResponse {
        status: 200,
        body: "{}".to_string(),
    }])
    .await;
    let client = OpenBaoClient::new(
        base,
        "secret".to_string(),
        "ns".to_string(),
        "tok".to_string(),
    );
    client
        .write_secret("p", &serde_json::json!({}))
        .await
        .expect("ok");
    let reqs = reqs.lock().await;
    let req = reqs.first().expect("req");
    assert!(req.headers.to_lowercase().contains("x-vault-namespace: ns"));
}

#[tokio::test]
async fn no_namespace_header_when_empty() {
    let (base, reqs) = start_mock_server(vec![MockResponse {
        status: 200,
        body: r#"{"data":{"keys":["a/"]}}"#.to_string(),
    }])
    .await;
    let client = OpenBaoClient::new(base, "secret".to_string(), "".to_string(), "t".to_string());
    client.list("p").await.expect("ok");
    let reqs = reqs.lock().await;
    let req = reqs.first().expect("req");
    assert!(!req.headers.to_lowercase().contains("x-vault-namespace"));
}
