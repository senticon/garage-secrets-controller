use serde_json::Value;

use crate::error::Result;

#[async_trait::async_trait]
pub trait KeyStorageProvider {
    #[allow(dead_code)]
    async fn list(&self, path: &str) -> Result<Vec<String>>;
    async fn read_secret_value(&self, path: &str) -> Result<Option<Value>>;
    async fn write_secret_value(&self, path: &str, data: &Value) -> Result<()>;
}

#[async_trait::async_trait]
pub trait KeyStorageMultiProvider {
    async fn list_multi(&self, path: &str, namespaces: &[String]) -> Result<Vec<String>>;
}
