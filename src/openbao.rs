use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{trace, warn};

use crate::error::{AppError, Result};
use crate::key_storage::KeyStorageProvider;

mod tests;

#[derive(Clone)]
pub struct OpenBaoClient {
    base: String,
    mount: String,
    token: String,
    client: reqwest::Client,
}

#[derive(Debug, Deserialize)]
struct ListResponse {
    data: ListData,
}

#[derive(Debug, Deserialize)]
struct ListData {
    keys: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ReadResponse<T> {
    data: ReadData<T>,
}

#[derive(Debug, Deserialize)]
struct ReadData<T> {
    data: T,
}

#[derive(Debug, Serialize)]
struct WriteRequest<'a, T> {
    data: &'a T,
}

impl OpenBaoClient {
    pub fn new(base: String, mount: String, token: String) -> Self {
        Self {
            base,
            mount,
            token,
            client: reqwest::Client::new(),
        }
    }

    pub async fn list(&self, path: &str) -> Result<Vec<String>> {
        let url = format!("{}/v1/{}/metadata/{}", self.base, self.mount, path);
        let resp = self
            .client
            .request(reqwest::Method::from_bytes(b"LIST").expect("method"), url)
            .header("X-Vault-Token", &self.token)
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(Vec::new());
        }
        ensure_success("metadata", path, status, &body)?;
        let parsed: ListResponse = parse_json("metadata", path, &body)?;
        Ok(parsed.data.keys)
    }

    pub async fn read_secret<T: DeserializeOwned>(&self, path: &str) -> Result<Option<T>> {
        let url = format!("{}/v1/{}/data/{}", self.base, self.mount, path);
        let resp = self
            .client
            .get(url)
            .header("X-Vault-Token", &self.token)
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        ensure_success("data", path, status, &body)?;
        let parsed: ReadResponse<T> = parse_json("data", path, &body)?;
        Ok(Some(parsed.data.data))
    }

    pub async fn write_secret<T: Serialize>(&self, path: &str, data: &T) -> Result<()> {
        let url = format!("{}/v1/{}/data/{}", self.base, self.mount, path);
        let req = WriteRequest { data };
        let resp = self
            .client
            .post(url)
            .header("X-Vault-Token", &self.token)
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .json(&req)
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        ensure_success("data", path, status, &body)?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl KeyStorageProvider for OpenBaoClient {
    async fn list(&self, path: &str) -> Result<Vec<String>> {
        self.list(path).await
    }

    async fn read_secret_value(&self, path: &str) -> Result<Option<Value>> {
        self.read_secret(path).await
    }

    async fn write_secret_value(&self, path: &str, data: &Value) -> Result<()> {
        self.write_secret(path, data).await
    }
}

fn ensure_success(
    endpoint: &str,
    path: &str,
    status: reqwest::StatusCode,
    body: &str,
) -> Result<()> {
    if status.is_success() {
        return Ok(());
    }
    warn!(endpoint = endpoint, path = %path, status = %status, body = body, "openbao non-success response");
    Err(AppError::OpenBaoApi {
        status: status.as_u16(),
        message: body.to_string(),
    })
}

fn parse_json<T: DeserializeOwned>(endpoint: &str, path: &str, body: &str) -> Result<T> {
    serde_json::from_str(body).map_err(|err| {
        trace!(endpoint = %endpoint, path = %path, body = %body, error = %err, "failed to parse openbao json body");
        AppError::Json(err)
    })
}
