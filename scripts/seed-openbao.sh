#!/usr/bin/env bash
set -euo pipefail

export BAO_ADDR="${BAO_ADDR:-http://127.0.0.1:8200}"
export BAO_TOKEN="${BAO_TOKEN:-root}"
export BAO_NAMESPACE="${BAO_NAMESPACE:-}"

PREFIX="${BAO_PREFIX:-garage}"
BUCKET_NAME="${GARAGE_BUCKET_NAME:-my-bucket}"
KEY_NAME="${GARAGE_KEY_NAME:-my-app-key}"
GRANT_NAME="${GARAGE_GRANT_NAME:-${KEY_NAME}--${BUCKET_NAME}}"
RECREATE="false"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --recreate)
      RECREATE="true"
      ;;
    *)
      echo "Unknown arg: $1" >&2
      exit 1
      ;;
  esac
  shift
done

# Determine which namespaces to target
ns_list=()
if [ -z "${BAO_NAMESPACE}" ]; then
  ns_list=("_" "default" "test")
else
  ns_list=("${BAO_NAMESPACE}")
fi

seed_ns() {
  local ns="$1"
  local ns_flag=""
  if [ -n "${ns}" ] && [ "${ns}" != "_" ]; then
    ns_flag="-namespace=${ns}"
  fi

  if [ "${RECREATE}" = "true" ]; then
    rm -f /dev/null
    echo "Recreate mode: deleting existing OpenBao KV records in ${ns:-root} namespace"
    bao kv metadata delete "kv/${PREFIX}/grants/${GRANT_NAME}" ${ns_flag} >/dev/null 2>&1 || true
    bao kv metadata delete "kv/${PREFIX}/keys/${KEY_NAME}" ${ns_flag} >/dev/null 2>&1 || true
    bao kv metadata delete "kv/${PREFIX}/buckets/${BUCKET_NAME}" ${ns_flag} >/dev/null 2>&1 || true
  fi

  local kv_path="kv/${PREFIX}/buckets/${BUCKET_NAME}"
  bao kv put "${kv_path}" ${ns_flag} \
    name="${BUCKET_NAME}" \
    state=requested

  kv_path="kv/${PREFIX}/keys/${KEY_NAME}"
  bao kv put "${kv_path}" ${ns_flag} \
    name="${KEY_NAME}" \
    access_key_id= \
    secret_access_key= \
    state=requested

  kv_path="kv/${PREFIX}/grants/${GRANT_NAME}"
  bao kv put "${kv_path}" ${ns_flag} \
    key="${KEY_NAME}" \
    bucket="${BUCKET_NAME}" \
    read=true \
    write=true \
    owner=false \
    state=requested
}

echo "Seeding desired records"
for ns in "${ns_list[@]}"; do
  seed_ns "${ns}"
done

echo "Seeded desired records in all target namespaces"
