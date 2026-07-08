use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{info, trace, warn};

use crate::error::{AppError, Result};

mod tests;

#[derive(Clone)]
pub struct GarageClient {
    base: String,
    token: String,
    client: reqwest::Client,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Bucket {
    pub id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateBucketRequest<'a> {
    global_alias: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GarageKey {
    pub access_key_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub secret_access_key: Option<String>,
}

#[derive(Debug, Clone)]
pub enum KeyLookup {
    None,
    Single(GarageKey),
    Multiple,
}

#[async_trait::async_trait]
pub trait GarageApi {
    async fn get_status(&self) -> Result<Value>;
    async fn get_bucket_by_alias_or_name(&self, name: &str) -> Result<Option<Bucket>>;
    async fn create_bucket(&self, name: &str) -> Result<Bucket>;
    async fn lookup_key_by_name(&self, name: &str) -> Result<KeyLookup>;
    async fn create_key(&self, name: &str) -> Result<GarageKey>;
    async fn allow_bucket_key(
        &self,
        bucket_id: &str,
        access_key_id: &str,
        read: bool,
        write: bool,
        owner: bool,
    ) -> Result<()>;
}

#[derive(Debug, Serialize)]
struct CreateKeyRequest<'a> {
    name: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AllowRequest {
    bucket_id: String,
    access_key_id: String,
    permissions: Permissions,
}

#[derive(Debug, Serialize)]
struct Permissions {
    read: bool,
    write: bool,
    owner: bool,
}

impl GarageClient {
    pub fn new(base: String, token: String) -> Self {
        Self {
            base,
            token,
            client: reqwest::Client::new(),
        }
    }

    fn req(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.client
            .request(
                method,
                format!("{}/{}", self.base, path.trim_start_matches('/')),
            )
            .bearer_auth(&self.token)
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
    }

    pub async fn get_status(&self) -> Result<serde_json::Value> {
        let resp = self
            .req(reqwest::Method::GET, "/v2/GetClusterStatus")
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        ensure_success("/v2/GetClusterStatus", status, &body)?;
        parse_json_value("/v2/GetClusterStatus", &body)
    }

    pub async fn create_bucket(&self, name: &str) -> Result<Bucket> {
        let resp = self
            .req(reqwest::Method::POST, "/v2/CreateBucket")
            .json(&CreateBucketRequest { global_alias: name })
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        ensure_success("/v2/CreateBucket", status, &body)?;
        if !body.trim().is_empty() {
            let v: Value = parse_json_value("/v2/CreateBucket", &body)?;
            if let Some(bucket) = bucket_from_value(&v) {
                return Ok(bucket);
            }
        }

        self.get_bucket_by_alias_or_name(name)
            .await?
            .ok_or_else(|| {
                AppError::Resource(format!(
                    "bucket '{name}' was created but lookup by alias failed"
                ))
            })
    }

    pub async fn get_bucket_by_alias_or_name(&self, name: &str) -> Result<Option<Bucket>> {
        let resp = self
            .req(reqwest::Method::GET, "/v2/GetBucketInfo")
            .query(&[("search", name)])
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        ensure_success("/v2/GetBucketInfo", status, &body)?;
        let v: Value = parse_json_value("/v2/GetBucketInfo", &body)?;
        Ok(bucket_from_value(&v))
    }

    pub async fn create_key(&self, name: &str) -> Result<GarageKey> {
        let resp = self
            .req(reqwest::Method::POST, "/v2/CreateKey")
            .json(&CreateKeyRequest { name })
            .send()
            .await?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();

        ensure_success("/v2/CreateKey", status, &body)?;

        if !body.trim().is_empty() {
            let v: Value = parse_json_value("/v2/CreateKey", &body)?;
            if let Some(key) = key_from_value(&v) {
                return Ok(key);
            }
        }

        info!(endpoint = "/v2/CreateKey", name = %name);
        match self.lookup_key_by_name(name).await? {
            KeyLookup::Single(k) => Ok(k),
            KeyLookup::None => Err(AppError::Resource(format!(
                "key '{name}' was created but lookup failed"
            ))),
            KeyLookup::Multiple => Err(AppError::Resource(format!(
                "key '{name}' was created but multiple matches were found"
            ))),
        }
    }

    pub async fn lookup_key_by_name(&self, name: &str) -> Result<KeyLookup> {
        let resp = self
            .req(reqwest::Method::GET, "/v2/GetKeyInfo")
            .query(&[("search", name)])
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(KeyLookup::None);
        }
        ensure_success("/v2/GetKeyInfo", status, &body)?;

        let v: Value = parse_json_value("/v2/GetKeyInfo", &body)?;
        if let Some(key) = key_from_value(&v) {
            return Ok(KeyLookup::Single(key));
        }
        Ok(KeyLookup::None)
    }

    pub async fn allow_bucket_key(
        &self,
        bucket_id: &str,
        access_key_id: &str,
        read: bool,
        write: bool,
        owner: bool,
    ) -> Result<()> {
        let resp = self
            .req(reqwest::Method::POST, "/v2/AllowBucketKey")
            .json(&AllowRequest {
                bucket_id: bucket_id.to_string(),
                access_key_id: access_key_id.to_string(),
                permissions: Permissions { read, write, owner },
            })
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        ensure_success("/v2/AllowBucketKey", status, &body)?;
        info!(endpoint = "/v2/AllowBucketKey", bucket_id = %bucket_id, access_key_id = %access_key_id, read, write, owner);
        Ok(())
    }
}

#[async_trait::async_trait]
impl GarageApi for GarageClient {
    async fn get_status(&self) -> Result<Value> {
        self.get_status().await
    }

    async fn get_bucket_by_alias_or_name(&self, name: &str) -> Result<Option<Bucket>> {
        self.get_bucket_by_alias_or_name(name).await
    }

    async fn create_bucket(&self, name: &str) -> Result<Bucket> {
        self.create_bucket(name).await
    }

    async fn lookup_key_by_name(&self, name: &str) -> Result<KeyLookup> {
        self.lookup_key_by_name(name).await
    }

    async fn create_key(&self, name: &str) -> Result<GarageKey> {
        self.create_key(name).await
    }

    async fn allow_bucket_key(
        &self,
        bucket_id: &str,
        access_key_id: &str,
        read: bool,
        write: bool,
        owner: bool,
    ) -> Result<()> {
        self.allow_bucket_key(bucket_id, access_key_id, read, write, owner)
            .await
    }
}

fn ensure_success(endpoint: &str, status: reqwest::StatusCode, body: &str) -> Result<()> {
    if status.is_success() {
        return Ok(());
    }
    warn!(endpoint = endpoint, status = %status, body = body, "garage non-success response");
    Err(AppError::GarageApi {
        status: status.as_u16(),
        message: body.to_string(),
    })
}

fn parse_json_value(endpoint: &str, body: &str) -> Result<Value> {
    serde_json::from_str(body).map_err(|err| {
        trace!(endpoint = %endpoint, body = %body, error = %err, "failed to parse garage json body");
        AppError::Json(err)
    })
}

fn first_obj(v: &Value) -> Option<&Value> {
    if v.is_object() {
        return Some(v);
    }
    if let Some(arr) = v.as_array() {
        return arr.first();
    }
    v.get("items")?.as_array()?.first()
}

fn bucket_from_value(v: &Value) -> Option<Bucket> {
    let obj = first_obj(v)?;
    let id = obj.get("id")?.as_str()?.to_string();
    Some(Bucket { id })
}

fn key_from_value(v: &Value) -> Option<GarageKey> {
    let obj = first_obj(v)?;
    let access_key_id = obj
        .get("accessKeyId")
        .and_then(Value::as_str)
        .or_else(|| obj.get("id").and_then(Value::as_str))?
        .to_string();
    let secret_access_key = obj
        .get("secretAccessKey")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let name = obj
        .get("name")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    Some(GarageKey {
        access_key_id,
        name,
        secret_access_key,
    })
}
