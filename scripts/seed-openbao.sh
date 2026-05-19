#!/usr/bin/env bash
set -euo pipefail

export BAO_ADDR="${BAO_ADDR:-http://127.0.0.1:8200}"
export BAO_TOKEN="${BAO_TOKEN:-root}"

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

if [ "${RECREATE}" = "true" ]; then
  echo "Recreate mode: deleting existing OpenBao KV records"
  bao kv metadata delete "kv/${PREFIX}/grants/${GRANT_NAME}" >/dev/null 2>&1 || true
  bao kv metadata delete "kv/${PREFIX}/keys/${KEY_NAME}" >/dev/null 2>&1 || true
  bao kv metadata delete "kv/${PREFIX}/buckets/${BUCKET_NAME}" >/dev/null 2>&1 || true
fi

bao kv put "kv/${PREFIX}/buckets/${BUCKET_NAME}" \
  name="${BUCKET_NAME}" \
  state=requested

bao kv put "kv/${PREFIX}/keys/${KEY_NAME}" \
  name="${KEY_NAME}" \
  access_key_id= \
  secret_access_key= \
  state=requested

bao kv put "kv/${PREFIX}/grants/${GRANT_NAME}" \
  key="${KEY_NAME}" \
  bucket="${BUCKET_NAME}" \
  read=true \
  write=true \
  owner=false \
  state=requested

echo "Seeded desired records"
