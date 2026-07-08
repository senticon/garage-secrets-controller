use super::*;
use crate::garage::{GarageApi, GarageKey};
use crate::key_storage::{KeyStorageMultiProvider, KeyStorageProvider};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct MockKeyStore {
    inner: Arc<Mutex<MockKeyStoreState>>,
}

#[derive(Default)]
struct MockKeyStoreState {
    list_map: HashMap<String, Vec<String>>,
    secrets: HashMap<String, Value>,
    writes: Vec<(String, Value)>,
    list_errors: HashMap<String, String>,
    read_errors: HashMap<String, String>,
    write_errors: HashMap<String, String>,
}

#[async_trait::async_trait]
impl KeyStorageProvider for MockKeyStore {
    async fn list(&self, path: &str) -> Result<Vec<String>> {
        let inner = self.inner.lock().expect("lock");
        if let Some(msg) = inner.list_errors.get(path) {
            return Err(AppError::Resource(msg.clone()));
        }
        Ok(inner.list_map.get(path).cloned().unwrap_or_default())
    }

    async fn read_secret_value(&self, path: &str) -> Result<Option<Value>> {
        let inner = self.inner.lock().expect("lock");
        if let Some(msg) = inner.read_errors.get(path) {
            return Err(AppError::Resource(msg.clone()));
        }
        Ok(inner.secrets.get(path).cloned())
    }

    async fn write_secret_value(&self, path: &str, data: &Value) -> Result<()> {
        let mut inner = self.inner.lock().expect("lock");
        if let Some(msg) = inner.write_errors.get(path) {
            return Err(AppError::Resource(msg.clone()));
        }
        inner.writes.push((path.to_string(), data.clone()));
        inner.secrets.insert(path.to_string(), data.clone());
        Ok(())
    }
}

#[async_trait::async_trait]
impl KeyStorageMultiProvider for MockKeyStore {
    async fn list_multi(&self, path: &str, namespaces: &[String]) -> Result<Vec<String>> {
        let inner = self.inner.lock().expect("lock");
        let mut keys = HashMap::new();
        if namespaces.is_empty() {
            if let Some(msg) = inner.list_errors.get(path) {
                return Err(AppError::Resource(msg.clone()));
            }
            if let Some(found) = inner.list_map.get(path) {
                for k in found {
                    keys.insert(k.clone(), true);
                }
            }
        } else {
            for ns in namespaces {
                let namespaced = format!("{}/{}", ns, path);
                if let Some(msg) = inner.list_errors.get(&namespaced) {
                    return Err(AppError::Resource(msg.clone()));
                }
                if let Some(found) = inner.list_map.get(&namespaced) {
                    for k in found {
                        keys.insert(k.clone(), true);
                    }
                }
            }
        }
        Ok(keys.into_keys().collect())
    }
}

#[derive(Clone, Default)]
struct MockGarage {
    inner: Arc<Mutex<MockGarageState>>,
}

#[derive(Default)]
struct MockGarageState {
    buckets: HashMap<String, Bucket>,
    keys: HashMap<String, KeyLookup>,
    created_buckets: Vec<String>,
    created_keys: Vec<String>,
    allowed: Vec<(String, String, bool, bool, bool)>,
    fail_status: Option<String>,
    fail_create_bucket: Option<String>,
    fail_lookup_key: HashMap<String, String>,
    fail_create_key: Option<String>,
    fail_allow: Option<String>,
}

#[async_trait::async_trait]
impl GarageApi for MockGarage {
    async fn get_status(&self) -> Result<Value> {
        if let Some(msg) = &self.inner.lock().expect("lock").fail_status {
            return Err(AppError::Resource(msg.clone()));
        }
        Ok(json!({ "ok": true }))
    }

    async fn get_bucket_by_alias_or_name(&self, name: &str) -> Result<Option<Bucket>> {
        Ok(self.inner.lock().expect("lock").buckets.get(name).cloned())
    }

    async fn create_bucket(&self, name: &str) -> Result<Bucket> {
        let mut inner = self.inner.lock().expect("lock");
        if let Some(msg) = &inner.fail_create_bucket {
            return Err(AppError::Resource(msg.clone()));
        }
        inner.created_buckets.push(name.to_string());
        let bucket = Bucket {
            id: format!("bucket-{name}"),
        };
        inner.buckets.insert(name.to_string(), bucket.clone());
        Ok(bucket)
    }

    async fn lookup_key_by_name(&self, name: &str) -> Result<KeyLookup> {
        let inner = self.inner.lock().expect("lock");
        if let Some(msg) = inner.fail_lookup_key.get(name) {
            return Err(AppError::Resource(msg.clone()));
        }
        Ok(inner.keys.get(name).cloned().unwrap_or(KeyLookup::None))
    }

    async fn create_key(&self, name: &str) -> Result<GarageKey> {
        let mut inner = self.inner.lock().expect("lock");
        if let Some(msg) = &inner.fail_create_key {
            return Err(AppError::Resource(msg.clone()));
        }
        inner.created_keys.push(name.to_string());
        let key = GarageKey {
            access_key_id: format!("ak-{name}"),
            name: Some(name.to_string()),
            secret_access_key: Some(format!("sk-{name}")),
        };
        inner
            .keys
            .insert(name.to_string(), KeyLookup::Single(key.clone()));
        Ok(key)
    }

    async fn allow_bucket_key(
        &self,
        bucket_id: &str,
        access_key_id: &str,
        read: bool,
        write: bool,
        owner: bool,
    ) -> Result<()> {
        let mut inner = self.inner.lock().expect("lock");
        if let Some(msg) = &inner.fail_allow {
            return Err(AppError::Resource(msg.clone()));
        }
        inner.allowed.push((
            bucket_id.to_string(),
            access_key_id.to_string(),
            read,
            write,
            owner,
        ));
        Ok(())
    }
}

fn reconciler(key_store: MockKeyStore, garage: MockGarage) -> Reconciler<MockKeyStore, MockGarage> {
    Reconciler {
        key_store,
        garage,
        bao_prefix: "controller".to_string(),
        namespaces: Vec::new(),
        dry_run: false,
    }
}

#[test]
fn requested_like_states_are_reconciled() {
    assert!(is_requested(&None));
    assert!(is_requested(&Some(DesiredState::Requested)));
    assert!(is_requested(&Some(DesiredState::Error)));
    assert!(!is_requested(&Some(DesiredState::Ready)));
}

#[tokio::test]
async fn reconcile_once_creates_missing_bucket_key_and_grant() {
    let bao = MockKeyStore::default();
    {
        let mut state = bao.inner.lock().expect("lock");
        state
            .list_map
            .insert("controller/buckets".to_string(), vec!["demo".to_string()]);
        state
            .list_map
            .insert("controller/keys".to_string(), vec!["app".to_string()]);
        state.list_map.insert(
            "controller/grants".to_string(),
            vec!["app--demo".to_string()],
        );
        state.secrets.insert(
            "controller/buckets/demo".to_string(),
            json!({"state":"requested"}),
        );
        state.secrets.insert(
            "controller/keys/app".to_string(),
            json!({"state":"requested"}),
        );
        state.secrets.insert(
            "controller/grants/app--demo".to_string(),
            json!({"read":true,"write":false,"owner":false,"state":"requested"}),
        );
    }

    let garage = MockGarage::default();
    let r = reconciler(bao.clone(), garage.clone());
    r.reconcile_once().await.expect("reconcile");

    let g = garage.inner.lock().expect("lock");
    assert_eq!(g.created_buckets, vec!["demo"]);
    assert_eq!(g.created_keys, vec!["app"]);
    assert_eq!(g.allowed.len(), 1);

    let b = bao.inner.lock().expect("lock");
    assert_eq!(b.writes.len(), 3);
    assert_eq!(b.secrets["controller/buckets/demo"]["state"], "ready");
    assert_eq!(b.secrets["controller/keys/app"]["state"], "ready");
    assert_eq!(b.secrets["controller/grants/app--demo"]["state"], "ready");
}

#[tokio::test]
async fn existing_key_is_updated_not_duplicated() {
    let bao = MockKeyStore::default();
    {
        let mut state = bao.inner.lock().expect("lock");
        state
            .list_map
            .insert("controller/keys".to_string(), vec!["app".to_string()]);
        state.secrets.insert(
            "controller/keys/app".to_string(),
            json!({"name":"app","state":"requested"}),
        );
    }
    let garage = MockGarage::default();
    garage.inner.lock().expect("lock").keys.insert(
        "app".to_string(),
        KeyLookup::Single(GarageKey {
            access_key_id: "existing-ak".to_string(),
            name: Some("app".to_string()),
            secret_access_key: None,
        }),
    );

    let r = reconciler(bao.clone(), garage.clone());
    r.reconcile_keys().await.expect("keys");

    assert!(garage.inner.lock().expect("lock").created_keys.is_empty());
    let writes = &bao.inner.lock().expect("lock").writes;
    assert_eq!(writes.len(), 1);
    let (path, updated) = &writes[0];
    assert_eq!(path, "controller/keys/app");
    assert_eq!(updated["state"], "ready");
    assert_eq!(updated["access_key_id"], "existing-ak");
}

#[tokio::test]
async fn reconcile_grant_marks_error_when_bucket_missing() {
    let bao = MockKeyStore::default();
    {
        let mut state = bao.inner.lock().expect("lock");
        state.list_map.insert(
            "controller/grants".to_string(),
            vec!["app--missing".to_string()],
        );
        state.secrets.insert(
            "controller/grants/app--missing".to_string(),
            json!({"read":true,"write":false,"owner":false,"state":"requested"}),
        );
    }

    let garage = MockGarage::default();
    let r = reconciler(bao.clone(), garage);
    r.reconcile_grants()
        .await
        .expect("grants pass with error marking");

    let state = bao.inner.lock().expect("lock");
    let err = &state.secrets["controller/grants/app--missing"];
    assert_eq!(err["state"], "error");
    assert!(err["error_message"]
        .as_str()
        .expect("message")
        .contains("bucket 'missing' not found"));
}

#[tokio::test]
async fn dry_run_does_not_write_or_mutate_remote() {
    let bao = MockKeyStore::default();
    {
        let mut state = bao.inner.lock().expect("lock");
        state
            .list_map
            .insert("controller/buckets".to_string(), vec!["demo".to_string()]);
        state.secrets.insert(
            "controller/buckets/demo".to_string(),
            json!({"state":"requested"}),
        );
    }
    let garage = MockGarage::default();
    let mut r = reconciler(bao.clone(), garage.clone());
    r.dry_run = true;
    r.reconcile_buckets().await.expect("dry-run");

    assert!(bao.inner.lock().expect("lock").writes.is_empty());
    assert!(garage
        .inner
        .lock()
        .expect("lock")
        .created_buckets
        .is_empty());
}

#[tokio::test]
async fn key_record_in_ready_state_is_ignored() {
    let bao = MockKeyStore::default();
    {
        let mut state = bao.inner.lock().expect("lock");
        state
            .list_map
            .insert("controller/keys".to_string(), vec!["app".to_string()]);
        state.secrets.insert(
            "controller/keys/app".to_string(),
            json!({"name":"app","state":"ready"}),
        );
    }
    let garage = MockGarage::default();

    reconciler(bao.clone(), garage.clone())
        .reconcile_keys()
        .await
        .expect("keys");

    assert!(garage.inner.lock().expect("lock").created_keys.is_empty());
    assert!(bao.inner.lock().expect("lock").writes.is_empty());
}

#[tokio::test]
async fn bucket_record_in_ready_state_is_ignored() {
    let key_store = MockKeyStore::default();
    {
        let mut state = key_store.inner.lock().expect("lock");
        state
            .list_map
            .insert("controller/buckets".to_string(), vec!["demo".to_string()]);
        state.secrets.insert(
            "controller/buckets/demo".to_string(),
            json!({"name":"demo","state":"ready"}),
        );
    }
    let garage = MockGarage::default();

    reconciler(key_store.clone(), garage.clone())
        .reconcile_buckets()
        .await
        .expect("buckets");

    assert!(garage
        .inner
        .lock()
        .expect("lock")
        .created_buckets
        .is_empty());
    assert!(key_store.inner.lock().expect("lock").writes.is_empty());
}

#[tokio::test]
async fn missing_key_secret_is_ignored() {
    let key_store = MockKeyStore::default();
    key_store
        .inner
        .lock()
        .expect("lock")
        .list_map
        .insert("controller/keys".to_string(), vec!["missing".to_string()]);

    reconciler(key_store.clone(), MockGarage::default())
        .reconcile_keys()
        .await
        .expect("keys");

    assert!(key_store.inner.lock().expect("lock").writes.is_empty());
}

#[tokio::test]
async fn dry_run_key_does_not_create_or_write() {
    let key_store = MockKeyStore::default();
    {
        let mut state = key_store.inner.lock().expect("lock");
        state
            .list_map
            .insert("controller/keys".to_string(), vec!["app".to_string()]);
        state.secrets.insert(
            "controller/keys/app".to_string(),
            json!({"state":"requested"}),
        );
    }
    let garage = MockGarage::default();
    let mut r = reconciler(key_store.clone(), garage.clone());
    r.dry_run = true;

    r.reconcile_keys().await.expect("dry-run key");

    assert!(garage.inner.lock().expect("lock").created_keys.is_empty());
    assert!(key_store.inner.lock().expect("lock").writes.is_empty());
}

#[tokio::test]
async fn multiple_matching_keys_are_skipped() {
    let bao = MockKeyStore::default();
    {
        let mut state = bao.inner.lock().expect("lock");
        state
            .list_map
            .insert("controller/keys".to_string(), vec!["app".to_string()]);
        state.secrets.insert(
            "controller/keys/app".to_string(),
            json!({"name":"app","state":"requested"}),
        );
    }
    let garage = MockGarage::default();
    garage
        .inner
        .lock()
        .expect("lock")
        .keys
        .insert("app".to_string(), KeyLookup::Multiple);

    reconciler(bao.clone(), garage.clone())
        .reconcile_keys()
        .await
        .expect("keys");

    assert!(garage.inner.lock().expect("lock").created_keys.is_empty());
    assert!(bao.inner.lock().expect("lock").writes.is_empty());
}

#[tokio::test]
async fn invalid_bucket_record_is_marked_error() {
    let bao = MockKeyStore::default();
    {
        let mut state = bao.inner.lock().expect("lock");
        state
            .list_map
            .insert("controller/buckets".to_string(), vec!["bad".to_string()]);
        state.secrets.insert(
            "controller/buckets/bad".to_string(),
            json!({"state":"requested","name":42}),
        );
    }

    reconciler(bao.clone(), MockGarage::default())
        .reconcile_buckets()
        .await
        .expect("buckets");

    let state = bao.inner.lock().expect("lock");
    let err = &state.secrets["controller/buckets/bad"];
    assert_eq!(err["state"], "error");
    assert!(err["error_message"]
        .as_str()
        .expect("message")
        .contains("decode bucket record"));
}

#[tokio::test]
async fn non_object_existing_error_payload_is_replaced_during_mark_error() {
    let bao = MockKeyStore::default();
    {
        let mut state = bao.inner.lock().expect("lock");
        state
            .list_map
            .insert("controller/buckets".to_string(), vec!["boom".to_string()]);
        state.secrets.insert(
            "controller/buckets/boom".to_string(),
            json!("not-an-object"),
        );
    }
    let garage = MockGarage::default();
    garage.inner.lock().expect("lock").fail_create_bucket = Some("create failed".to_string());

    reconciler(bao.clone(), garage)
        .reconcile_buckets()
        .await
        .expect("buckets");

    let state = bao.inner.lock().expect("lock");
    let err = &state.secrets["controller/buckets/boom"];
    assert_eq!(err["state"], "error");
    assert!(err["error_message"]
        .as_str()
        .expect("message")
        .contains("decode bucket record"));
}

#[tokio::test]
async fn reconcile_once_fails_fast_when_garage_status_fails() {
    let bao = MockKeyStore::default();
    let garage = MockGarage::default();
    garage.inner.lock().expect("lock").fail_status = Some("status down".to_string());

    let err = reconciler(bao.clone(), garage)
        .reconcile_once()
        .await
        .expect_err("status failure should bubble up");
    assert!(err.to_string().contains("status down"));
    assert!(bao.inner.lock().expect("lock").writes.is_empty());
}

#[tokio::test]
async fn grant_allow_failure_is_marked_error() {
    let bao = MockKeyStore::default();
    {
        let mut state = bao.inner.lock().expect("lock");
        state.list_map.insert(
            "controller/grants".to_string(),
            vec!["app--demo".to_string()],
        );
        state.secrets.insert(
            "controller/grants/app--demo".to_string(),
            json!({"read":true,"write":true,"owner":false,"state":"requested"}),
        );
    }

    let garage = MockGarage::default();
    {
        let mut g = garage.inner.lock().expect("lock");
        g.buckets.insert(
            "demo".to_string(),
            Bucket {
                id: "bucket-1".to_string(),
            },
        );
        g.keys.insert(
            "app".to_string(),
            KeyLookup::Single(GarageKey {
                access_key_id: "ak-1".to_string(),
                name: Some("app".to_string()),
                secret_access_key: None,
            }),
        );
        g.fail_allow = Some("allow failed".to_string());
    }

    reconciler(bao.clone(), garage)
        .reconcile_grants()
        .await
        .expect("grants");

    let state = bao.inner.lock().expect("lock");
    let err = &state.secrets["controller/grants/app--demo"];
    assert_eq!(err["state"], "error");
    assert!(err["error_message"]
        .as_str()
        .expect("message")
        .contains("allow failed"));
}

#[tokio::test]
async fn provider_list_failure_bubbles_up() {
    let key_store = MockKeyStore::default();
    key_store
        .inner
        .lock()
        .expect("lock")
        .list_errors
        .insert("controller/buckets".to_string(), "list failed".to_string());

    let err = reconciler(key_store, MockGarage::default())
        .reconcile_buckets()
        .await
        .expect_err("list error should bubble up");

    assert!(err.to_string().contains("list failed"));
}

#[tokio::test]
async fn missing_secret_is_ignored() {
    let key_store = MockKeyStore::default();
    key_store.inner.lock().expect("lock").list_map.insert(
        "controller/buckets".to_string(),
        vec!["missing".to_string()],
    );

    reconciler(key_store.clone(), MockGarage::default())
        .reconcile_buckets()
        .await
        .expect("missing record ignored");

    assert!(key_store.inner.lock().expect("lock").writes.is_empty());
}

#[tokio::test]
async fn missing_grant_secret_is_ignored() {
    let key_store = MockKeyStore::default();
    key_store.inner.lock().expect("lock").list_map.insert(
        "controller/grants".to_string(),
        vec!["app--demo".to_string()],
    );

    reconciler(key_store.clone(), MockGarage::default())
        .reconcile_grants()
        .await
        .expect("grants");

    assert!(key_store.inner.lock().expect("lock").writes.is_empty());
}

#[tokio::test]
async fn existing_bucket_is_marked_ready_without_creating() {
    let key_store = MockKeyStore::default();
    {
        let mut state = key_store.inner.lock().expect("lock");
        state
            .list_map
            .insert("controller/buckets".to_string(), vec!["demo".to_string()]);
        state.secrets.insert(
            "controller/buckets/demo".to_string(),
            json!({"name":"demo","state":"requested"}),
        );
    }
    let garage = MockGarage::default();
    garage.inner.lock().expect("lock").buckets.insert(
        "demo".to_string(),
        Bucket {
            id: "bucket-existing".to_string(),
        },
    );

    reconciler(key_store.clone(), garage.clone())
        .reconcile_buckets()
        .await
        .expect("bucket ready");

    assert!(garage
        .inner
        .lock()
        .expect("lock")
        .created_buckets
        .is_empty());
    assert_eq!(
        key_store.inner.lock().expect("lock").secrets["controller/buckets/demo"]
            ["garage_bucket_id"],
        "bucket-existing"
    );
}

#[tokio::test]
async fn bucket_create_failure_is_marked_error() {
    let key_store = MockKeyStore::default();
    {
        let mut state = key_store.inner.lock().expect("lock");
        state
            .list_map
            .insert("controller/buckets".to_string(), vec!["demo".to_string()]);
        state.secrets.insert(
            "controller/buckets/demo".to_string(),
            json!({"state":"requested"}),
        );
    }
    let garage = MockGarage::default();
    garage.inner.lock().expect("lock").fail_create_bucket = Some("create failed".to_string());

    reconciler(key_store.clone(), garage)
        .reconcile_buckets()
        .await
        .expect("bucket error marked");

    let state = key_store.inner.lock().expect("lock");
    assert_eq!(state.secrets["controller/buckets/demo"]["state"], "error");
    assert!(state.secrets["controller/buckets/demo"]["error_message"]
        .as_str()
        .expect("message")
        .contains("create bucket 'demo'"));
}

#[tokio::test]
async fn mark_error_write_failure_bubbles_up() {
    let key_store = MockKeyStore::default();
    {
        let mut state = key_store.inner.lock().expect("lock");
        state
            .list_map
            .insert("controller/buckets".to_string(), vec!["bad".to_string()]);
        state.secrets.insert(
            "controller/buckets/bad".to_string(),
            json!({"state":"requested","name":42}),
        );
        state.write_errors.insert(
            "controller/buckets/bad".to_string(),
            "write failed".to_string(),
        );
    }

    let err = reconciler(key_store, MockGarage::default())
        .reconcile_buckets()
        .await
        .expect_err("mark error write failure should bubble up");

    assert!(err.to_string().contains("write failed"));
}

#[tokio::test]
async fn direct_secret_write_failure_is_wrapped_for_keys() {
    let key_store = MockKeyStore::default();
    {
        let mut state = key_store.inner.lock().expect("lock");
        state
            .list_map
            .insert("controller/keys".to_string(), vec!["app".to_string()]);
        state.secrets.insert(
            "controller/keys/app".to_string(),
            json!({"state":"requested"}),
        );
        state.write_errors.insert(
            "controller/keys/app".to_string(),
            "write failed".to_string(),
        );
    }

    let err = reconciler(key_store.clone(), MockGarage::default())
        .reconcile_key_path("controller/keys/app")
        .await
        .expect_err("key write error should bubble up");

    assert!(err.to_string().contains("write key status"));
}

#[tokio::test]
async fn read_key_failure_is_marked_error() {
    let key_store = MockKeyStore::default();
    {
        let mut state = key_store.inner.lock().expect("lock");
        state
            .list_map
            .insert("controller/keys".to_string(), vec!["app".to_string()]);
        state
            .read_errors
            .insert("controller/keys/app".to_string(), "read failed".to_string());
    }

    let err = reconciler(key_store, MockGarage::default())
        .reconcile_keys()
        .await
        .expect_err("read failure should bubble up when error marking cannot read");

    assert!(err.to_string().contains("read failed"));
}

#[tokio::test]
async fn invalid_key_record_is_marked_error() {
    let key_store = MockKeyStore::default();
    {
        let mut state = key_store.inner.lock().expect("lock");
        state
            .list_map
            .insert("controller/keys".to_string(), vec!["bad".to_string()]);
        state.secrets.insert(
            "controller/keys/bad".to_string(),
            json!({"name":42,"state":"requested"}),
        );
    }

    reconciler(key_store.clone(), MockGarage::default())
        .reconcile_keys()
        .await
        .expect("invalid key marked");

    assert!(
        key_store.inner.lock().expect("lock").secrets["controller/keys/bad"]["error_message"]
            .as_str()
            .expect("message")
            .contains("decode key record")
    );
}

#[tokio::test]
async fn key_lookup_and_create_failures_are_marked_error() {
    let key_store = MockKeyStore::default();
    {
        let mut state = key_store.inner.lock().expect("lock");
        state.list_map.insert(
            "controller/keys".to_string(),
            vec!["lookup".to_string(), "create".to_string()],
        );
        state.secrets.insert(
            "controller/keys/lookup".to_string(),
            json!({"name":"lookup","state":"requested"}),
        );
        state.secrets.insert(
            "controller/keys/create".to_string(),
            json!({"name":"create","state":"requested"}),
        );
    }
    let garage = MockGarage::default();
    {
        let mut g = garage.inner.lock().expect("lock");
        g.fail_lookup_key
            .insert("lookup".to_string(), "lookup failed".to_string());
        g.fail_create_key = Some("create failed".to_string());
    }

    reconciler(key_store.clone(), garage)
        .reconcile_keys()
        .await
        .expect("key errors marked");

    let state = key_store.inner.lock().expect("lock");
    assert!(state.secrets["controller/keys/lookup"]["error_message"]
        .as_str()
        .expect("message")
        .contains("lookup key 'lookup'"));
    assert!(state.secrets["controller/keys/create"]["error_message"]
        .as_str()
        .expect("message")
        .contains("create key 'create'"));
}

#[tokio::test]
async fn grant_ready_state_is_ignored() {
    let key_store = MockKeyStore::default();
    {
        let mut state = key_store.inner.lock().expect("lock");
        state.list_map.insert(
            "controller/grants".to_string(),
            vec!["app--demo".to_string()],
        );
        state.secrets.insert(
            "controller/grants/app--demo".to_string(),
            json!({"key":"app","bucket":"demo","read":true,"write":false,"owner":false,"state":"ready"}),
        );
    }
    let garage = MockGarage::default();

    reconciler(key_store.clone(), garage.clone())
        .reconcile_grants()
        .await
        .expect("ready grant ignored");

    assert!(garage.inner.lock().expect("lock").allowed.is_empty());
    assert!(key_store.inner.lock().expect("lock").writes.is_empty());
}

#[tokio::test]
async fn grant_key_missing_and_multiple_are_marked_error() {
    let key_store = MockKeyStore::default();
    {
        let mut state = key_store.inner.lock().expect("lock");
        state.list_map.insert(
            "controller/grants".to_string(),
            vec!["missing--demo".to_string(), "multi--demo".to_string()],
        );
        state.secrets.insert(
            "controller/grants/missing--demo".to_string(),
            json!({"read":true,"write":false,"owner":false,"state":"requested"}),
        );
        state.secrets.insert(
            "controller/grants/multi--demo".to_string(),
            json!({"read":true,"write":false,"owner":false,"state":"requested"}),
        );
    }
    let garage = MockGarage::default();
    {
        let mut g = garage.inner.lock().expect("lock");
        g.buckets.insert(
            "demo".to_string(),
            Bucket {
                id: "bucket-1".to_string(),
            },
        );
        g.keys.insert("multi".to_string(), KeyLookup::Multiple);
    }

    reconciler(key_store.clone(), garage)
        .reconcile_grants()
        .await
        .expect("grant errors marked");

    let state = key_store.inner.lock().expect("lock");
    assert!(
        state.secrets["controller/grants/missing--demo"]["error_message"]
            .as_str()
            .expect("message")
            .contains("key 'missing' not found")
    );
    assert!(
        state.secrets["controller/grants/multi--demo"]["error_message"]
            .as_str()
            .expect("message")
            .contains("multiple keys found")
    );
}

#[tokio::test]
async fn dry_run_grant_does_not_mutate_remote_or_write() {
    let key_store = MockKeyStore::default();
    {
        let mut state = key_store.inner.lock().expect("lock");
        state.list_map.insert(
            "controller/grants".to_string(),
            vec!["app--demo".to_string()],
        );
        state.secrets.insert(
            "controller/grants/app--demo".to_string(),
            json!({"read":true,"write":true,"owner":true,"state":"requested"}),
        );
    }
    let garage = MockGarage::default();
    {
        let mut g = garage.inner.lock().expect("lock");
        g.buckets.insert(
            "demo".to_string(),
            Bucket {
                id: "bucket-1".to_string(),
            },
        );
        g.keys.insert(
            "app".to_string(),
            KeyLookup::Single(GarageKey {
                access_key_id: "ak-1".to_string(),
                name: Some("app".to_string()),
                secret_access_key: None,
            }),
        );
    }
    let mut r = reconciler(key_store.clone(), garage.clone());
    r.dry_run = true;

    r.reconcile_grants().await.expect("dry-run grant");

    assert!(garage.inner.lock().expect("lock").allowed.is_empty());
    assert!(key_store.inner.lock().expect("lock").writes.is_empty());
}

#[tokio::test]
async fn malformed_grant_name_is_marked_error() {
    let key_store = MockKeyStore::default();
    {
        let mut state = key_store.inner.lock().expect("lock");
        state.list_map.insert(
            "controller/grants".to_string(),
            vec!["malformed".to_string()],
        );
        state.secrets.insert(
            "controller/grants/malformed".to_string(),
            json!({"read":true,"write":false,"owner":false,"state":"requested"}),
        );
    }

    reconciler(key_store.clone(), MockGarage::default())
        .reconcile_grants()
        .await
        .expect("malformed grant marked");

    assert!(
        key_store.inner.lock().expect("lock").secrets["controller/grants/malformed"]
            ["error_message"]
            .as_str()
            .expect("message")
            .contains("decode grant record")
    );
}

#[tokio::test]
async fn non_object_grant_payload_with_inferable_name_is_marked_error() {
    let key_store = MockKeyStore::default();
    {
        let mut state = key_store.inner.lock().expect("lock");
        state.list_map.insert(
            "controller/grants".to_string(),
            vec!["app--demo".to_string()],
        );
        state.secrets.insert(
            "controller/grants/app--demo".to_string(),
            json!("not-an-object"),
        );
    }

    reconciler(key_store.clone(), MockGarage::default())
        .reconcile_grants()
        .await
        .expect("non-object grant marked");

    assert_eq!(
        key_store.inner.lock().expect("lock").secrets["controller/grants/app--demo"]["state"],
        "error"
    );
}

#[tokio::test]
async fn dry_run_error_marking_does_not_write() {
    let key_store = MockKeyStore::default();
    {
        let mut state = key_store.inner.lock().expect("lock");
        state
            .list_map
            .insert("controller/buckets".to_string(), vec!["bad".to_string()]);
        state.secrets.insert(
            "controller/buckets/bad".to_string(),
            json!({"name":42,"state":"requested"}),
        );
    }
    let mut r = reconciler(key_store.clone(), MockGarage::default());
    r.dry_run = true;

    r.reconcile_buckets().await.expect("dry-run error marking");

    assert!(key_store.inner.lock().expect("lock").writes.is_empty());
}

#[tokio::test]
async fn empty_path_final_segments_report_inference_errors() {
    let key_store = MockKeyStore::default();
    {
        let mut state = key_store.inner.lock().expect("lock");
        state.secrets.insert(
            "controller/buckets/".to_string(),
            json!({"state":"requested"}),
        );
        state
            .secrets
            .insert("controller/keys/".to_string(), json!({"state":"requested"}));
        state.secrets.insert(
            "controller/grants/".to_string(),
            json!({"read":true,"write":false,"owner":false,"state":"requested"}),
        );
    }
    let r = reconciler(key_store, MockGarage::default());

    let bucket_err = r
        .reconcile_bucket_path("controller/buckets/")
        .await
        .expect_err("bucket inference should fail");
    let key_err = r
        .reconcile_key_path("controller/keys/")
        .await
        .expect_err("key inference should fail");
    let grant_err = r
        .reconcile_grant_path("controller/grants/")
        .await
        .expect_err("grant inference should fail");

    assert!(bucket_err.to_string().contains("cannot infer bucket name"));
    assert!(key_err.to_string().contains("cannot infer key name"));
    assert!(grant_err.to_string().contains("cannot infer grant fields"));
}

#[tokio::test]
async fn multi_namespace_list_multi_merges_entries() {
    let key_store = MockKeyStore::default();
    {
        let mut state = key_store.inner.lock().expect("lock");
        state.list_map.insert(
            "ns1/controller/buckets".to_string(),
            vec!["b1".to_string(), "b2".to_string()],
        );
        state.list_map.insert(
            "ns2/controller/buckets".to_string(),
            vec!["b2".to_string(), "b3".to_string()],
        );
    }
    let r = reconciler(key_store.clone(), MockGarage::default());
    let namespaces = vec!["ns1".to_string(), "ns2".to_string()];

    let keys = key_store
        .list_multi("controller/buckets", &namespaces)
        .await
        .expect("multi list");

    assert_eq!(keys.len(), 3);
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(
        sorted,
        vec!["b1".to_string(), "b2".to_string(), "b3".to_string()]
    );
}

#[tokio::test]
async fn multi_namespace_empty_list_returns_root() {
    let key_store = MockKeyStore::default();
    {
        let mut state = key_store.inner.lock().expect("lock");
        state
            .list_map
            .insert("controller/buckets".to_string(), vec!["x".to_string()]);
    }
    let r = reconciler(key_store.clone(), MockGarage::default());

    let keys = key_store
        .list_multi("controller/buckets", &[])
        .await
        .expect("multi list");

    assert_eq!(keys, vec!["x".to_string()]);
}

#[tokio::test]
async fn all_path_prefix_named_namespace() {
    let paths = super::all_path(&["custo-ns".to_string()], "garage/buckets/my-bucket");
    assert_eq!(paths, "custo-ns/garage/buckets/my-bucket");
}

#[tokio::test]
async fn all_path_empty_namespace_original_path() {
    let paths = super::all_path(&[], "garage/buckets/my-bucket");
    assert_eq!(paths, "garage/buckets/my-bucket");
}
