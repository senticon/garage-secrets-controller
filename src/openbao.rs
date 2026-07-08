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
    namespace: String,
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
    pub fn new(base: String, mount: String, namespace: String, token: String) -> Self {
        Self {
            base,
            mount,
            namespace,
            token,
            client: reqwest::Client::new(),
        }
    }
    fn at_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("X-Vault-Token", self.token.parse().unwrap());
        headers.insert(
            reqwest::header::ACCEPT_ENCODING,
            "identity".parse().unwrap(),
        );
        if !self.namespace.is_empty() {
            headers.insert("X-Vault-Namespace", self.namespace.parse().unwrap());
        }
        headers
    }

    pub async fn list(&self, path: &str) -> Result<Vec<String>> {
        let url = format!("{}/v1/{}/metadata/{}", self.base, self.mount, path);
        let resp = self
            .client
            .request(reqwest::Method::from_bytes(b"LIST").expect("method"), url)
            .headers(self.at_headers())
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
            .headers(self.at_headers())
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
            .headers(self.at_headers())
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

mod multimap {
    use std::collections::HashMap;

    use crate::error::Result;
    use crate::key_storage::KeyStorageMultiProvider;

    #[async_trait::async_trait]
    impl KeyStorageMultiProvider for super::OpenBaoClient {
        async fn list_multi(&self, path: &str, namespaces: &[String]) -> Result<Vec<String>> {
            let mut seen = HashMap::new();
            if namespaces.is_empty() {
                let keys = self.list(path).await?;
                for k in keys {
                    seen.entry(k.clone()).or_insert(true);
                }
            } else {
                for ns in namespaces {
                    let client_with_ns = super::OpenBaoClient::new(
                        self.base.clone(),
                        self.mount.clone(),
                        ns.clone(),
                        self.token.clone(),
                    );
                    let keys = client_with_ns.list(path).await?;
                    for k in keys {
                        seen.entry(k.clone()).or_insert(true);
                    }
                }
            }
            Ok(seen.into_keys().collect())
        }
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
