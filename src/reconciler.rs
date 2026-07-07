use chrono::Utc;
use serde_json::{json, Value};
use tracing::{error, info, warn};

use crate::error::{AppError, Result};
use crate::garage::{Bucket, GarageApi, KeyLookup};
use crate::key_storage::KeyStorageProvider;
use crate::model::{is_requested, BucketRecord, DesiredState, GrantRecord, KeyRecord};

#[derive(Clone)]
pub struct Reconciler<K: KeyStorageProvider + Clone, G: GarageApi + Clone> {
    pub key_store: K,
    pub garage: G,
    pub prefix: String,
    pub dry_run: bool,
}

impl<K: KeyStorageProvider + Clone, G: GarageApi + Clone> Reconciler<K, G> {
    pub async fn reconcile_once(&self) -> Result<()> {
        let _ = self.garage.get_status().await?;
        self.reconcile_buckets().await?;
        self.reconcile_keys().await?;
        self.reconcile_grants().await?;
        Ok(())
    }

    async fn reconcile_buckets(&self) -> Result<()> {
        let entries = self
            .key_store
            .list(&format!("{}/buckets", self.prefix))
            .await?;
        for name in entries {
            let path = format!("{}/buckets/{}", self.prefix, name.trim_end_matches('/'));
            if let Err(err) = self.reconcile_bucket_path(&path).await {
                error!(path = %path, error = %err, "bucket reconcile failed");
                self.mark_error(&path, err).await?;
            }
        }
        Ok(())
    }

    async fn reconcile_bucket_path(&self, path: &str) -> Result<()> {
        let mut raw: Value = match self.key_store.read_secret_value(path).await? {
            Some(v) => v,
            None => return Ok(()),
        };

        if raw.get("name").is_none() {
            let inferred = path
                .split('/')
                .next_back()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    AppError::Resource(format!(
                        "cannot infer bucket name from path '{}': missing final segment",
                        path
                    ))
                })?;
            if let Some(obj) = raw.as_object_mut() {
                obj.insert("name".to_string(), Value::String(inferred.to_string()));
            }
        }

        let mut rec: BucketRecord = serde_json::from_value(raw).map_err(|e| {
            AppError::Resource(format!(
                "decode bucket record '{}': json error: {}",
                path, e
            ))
        })?;
        if !is_requested(&rec.state) {
            return Ok(());
        }
        let bucket = match self
            .garage
            .get_bucket_by_alias_or_name(&rec.name)
            .await
            .map_err(|e| AppError::Resource(format!("lookup bucket '{}': {e}", rec.name)))?
        {
            Some(b) => b,
            None => {
                if self.dry_run {
                    info!(bucket = %rec.name, "dry-run create bucket");
                    Bucket {
                        id: "dry-run-bucket-id".to_string(),
                    }
                } else {
                    self.garage.create_bucket(&rec.name).await.map_err(|e| {
                        AppError::Resource(format!("create bucket '{}': {e}", rec.name))
                    })?
                }
            }
        };
        rec.state = Some(DesiredState::Ready);
        rec.garage_bucket_id = Some(bucket.id);
        rec.error_message = None;
        rec.updated_at = Some(Utc::now());
        if self.dry_run {
            info!(path = %path, "dry-run write bucket ready state");
        } else {
            self.write_secret(path, &rec).await?;
        }
        Ok(())
    }

    async fn reconcile_keys(&self) -> Result<()> {
        let entries = self
            .key_store
            .list(&format!("{}/keys", self.prefix))
            .await?;
        for name in entries {
            let path = format!("{}/keys/{}", self.prefix, name.trim_end_matches('/'));
            if let Err(err) = self.reconcile_key_path(&path).await {
                error!(path = %path, error = %err, "key reconcile failed");
                self.mark_error(&path, err).await?;
            }
        }
        Ok(())
    }

    async fn reconcile_key_path(&self, path: &str) -> Result<()> {
        let mut raw: Value = match self
            .key_store
            .read_secret_value(path)
            .await
            .map_err(|e| AppError::Resource(format!("read key record '{}': {e}", path)))?
        {
            Some(v) => v,
            None => return Ok(()),
        };

        if raw.get("name").is_none() {
            let inferred = path
                .split('/')
                .next_back()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    AppError::Resource(format!(
                        "cannot infer key name from path '{}': missing final segment",
                        path
                    ))
                })?;
            if let Some(obj) = raw.as_object_mut() {
                obj.insert("name".to_string(), Value::String(inferred.to_string()));
            }
        }

        let mut rec: KeyRecord = serde_json::from_value(raw).map_err(|e| {
            AppError::Resource(format!("decode key record '{}': json error: {}", path, e))
        })?;
        if !is_requested(&rec.state) {
            return Ok(());
        }
        let lookup = self
            .garage
            .lookup_key_by_name(&rec.name)
            .await
            .map_err(|e| AppError::Resource(format!("lookup key '{}': {e}", rec.name)))?;

        match lookup {
            KeyLookup::Single(existing) => {
                warn!(key = %rec.name, access_key_id = %existing.access_key_id, path = %path, "key already exists in Garage; updating key record from existing key");
                rec.access_key_id = Some(existing.access_key_id);
                rec.secret_access_key = existing.secret_access_key;
            }
            KeyLookup::Multiple => {
                warn!(key = %rec.name, path = %path, "multiple Garage keys match name; skipping reconciliation for this key record");
                return Ok(());
            }
            KeyLookup::None => {
                if self.dry_run {
                    info!(key = %rec.name, "dry-run create key");
                    rec.access_key_id = Some("dry-run-access-key-id".to_string());
                    rec.secret_access_key = Some("dry-run-secret-access-key".to_string());
                } else {
                    let created = self
                        .garage
                        .create_key(&rec.name)
                        .await
                        .map_err(|e| AppError::Resource(format!("create key '{}': {e}", rec.name)))?;
                    rec.access_key_id = Some(created.access_key_id);
                    rec.secret_access_key = created.secret_access_key;
                }
            }
        }
        rec.state = Some(DesiredState::Ready);
        rec.error_message = None;
        rec.updated_at = Some(Utc::now());
        if self.dry_run {
            info!(path = %path, "dry-run write key ready state");
        } else {
            self.write_secret(path, &rec)
                .await
                .map_err(|e| AppError::Resource(format!("write key status '{}': {e}", path)))?;
        }
        Ok(())
    }

    async fn reconcile_grants(&self) -> Result<()> {
        let entries = self
            .key_store
            .list(&format!("{}/grants", self.prefix))
            .await?;
        for name in entries {
            let path = format!("{}/grants/{}", self.prefix, name.trim_end_matches('/'));
            if let Err(err) = self.reconcile_grant_path(&path).await {
                error!(path = %path, error = %err, "grant reconcile failed");
                self.mark_error(&path, err).await?;
            }
        }
        Ok(())
    }

    async fn reconcile_grant_path(&self, path: &str) -> Result<()> {
        let mut raw: Value = match self.key_store.read_secret_value(path).await? {
            Some(v) => v,
            None => return Ok(()),
        };

        if raw.get("key").is_none() || raw.get("bucket").is_none() {
            let inferred = path
                .split('/')
                .next_back()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    AppError::Resource(format!(
                        "cannot infer grant fields from path '{}': missing final segment",
                        path
                    ))
                })?;
            let parts: Vec<&str> = inferred.splitn(2, "--").collect();
            if parts.len() == 2 {
                if let Some(obj) = raw.as_object_mut() {
                    if obj.get("key").is_none() {
                        obj.insert("key".to_string(), Value::String(parts[0].to_string()));
                    }
                    if obj.get("bucket").is_none() {
                        obj.insert("bucket".to_string(), Value::String(parts[1].to_string()));
                    }
                }
            }
        }

        let mut rec: GrantRecord = serde_json::from_value(raw).map_err(|e| {
            AppError::Resource(format!("decode grant record '{}': json error: {}", path, e))
        })?;
        if !is_requested(&rec.state) {
            return Ok(());
        }
        let bucket = self
            .garage
            .get_bucket_by_alias_or_name(&rec.bucket)
            .await?
            .ok_or_else(|| AppError::Resource(format!("bucket '{}' not found", rec.bucket)))?;
        let key = self
            .garage
            .lookup_key_by_name(&rec.key)
            .await
            .map_err(|e| AppError::Resource(format!("lookup key '{}': {e}", rec.key)))?;
        let key = match key {
            KeyLookup::Single(k) => k,
            KeyLookup::None => {
                return Err(AppError::Resource(format!("key '{}' not found", rec.key)));
            }
            KeyLookup::Multiple => {
                return Err(AppError::Resource(format!(
                    "multiple keys found for '{}'; cannot apply grant safely",
                    rec.key
                )));
            }
        };
        if self.dry_run {
            info!(bucket = %rec.bucket, key = %rec.key, "dry-run allow bucket key");
        } else {
            self.garage
                .allow_bucket_key(
                    &bucket.id,
                    &key.access_key_id,
                    rec.read,
                    rec.write,
                    rec.owner,
                )
                .await?;
        }
        rec.state = Some(DesiredState::Ready);
        rec.error_message = None;
        rec.updated_at = Some(Utc::now());
        if self.dry_run {
            info!(path = %path, "dry-run write grant ready state");
        } else {
            self.write_secret(path, &rec).await?;
        }
        Ok(())
    }

    async fn mark_error(&self, path: &str, err: AppError) -> Result<()> {
        let patch = json!({
            "state": DesiredState::Error,
            "error_message": err.to_string(),
            "updated_at": Utc::now(),
        });
        if self.dry_run {
            info!(path = %path, "dry-run write error state");
            return Ok(());
        }

        let existing: Option<Value> = self.key_store.read_secret_value(path).await?;
        let mut merged = existing.unwrap_or_else(|| json!({}));
        if !merged.is_object() {
            merged = json!({});
        }

        let dst = merged.as_object_mut().expect("merged error payload object");
        let src = patch.as_object().expect("error patch object");
        for (k, v) in src {
            dst.insert(k.clone(), v.clone());
        }

        self.key_store.write_secret_value(path, &merged).await
    }

    async fn write_secret<T: serde::Serialize>(&self, path: &str, record: &T) -> Result<()> {
        let payload = serde_json::to_value(record)?;
        self.key_store.write_secret_value(path, &payload).await
    }
}

#[cfg(test)]
mod tests;
