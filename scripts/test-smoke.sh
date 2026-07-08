#!/usr/bin/env bash
set -euo pipefail

export BAO_ADDR="${BAO_ADDR:-http://127.0.0.1:8200}"
export BAO_TOKEN="${BAO_TOKEN:-root}"
export GARAGE_ADMIN_URL="${GARAGE_ADMIN_URL:-http://127.0.0.1:3903}"
export GARAGE_ADMIN_TOKEN="${GARAGE_ADMIN_TOKEN:-dev-garage-admin-token}"
RECREATE="${RECREATE:-false}"

docker compose up -d --build openbao garage

echo "Waiting for OpenBao"
until bao status >/dev/null 2>&1; do
  sleep 1
done

./scripts/init-openbao.sh
if [ "${RECREATE}" = "true" ]; then
  ./scripts/init-garage.sh --recreate
  ./scripts/seed-openbao.sh --recreate
else
  ./scripts/init-garage.sh
  ./scripts/seed-openbao.sh
fi

# Reconcile root namespace (no BAO_NAMESPACE set)
docker compose run --rm controller --once

test_ns() {
  local ns="$1"
  if [ -n "${ns}" ] && [ "${ns}" != "_" ]; then
    docker compose run --rm -e BAO_NAMESPACE="${ns}" controller --once
  fi

  local env_set=""
  if [ -n "${ns}" ] && [ "${ns}" != "_" ]; then
    export BAO_NAMESPACE="${ns}"
    env_set=" (BAO_NAMESPACE=${ns})"
  fi

  local label="Namespace '${ns:-root}'"

  local bucket_state key_state grant_state access_key_id secret_access_key
  bucket_state="$(bao kv get -format=json kv/${BAO_PREFIX:-garage}/buckets/${GARAGE_BUCKET_NAME:-my-bucket} | jq -r '.data.data.state')"
  key_state="$(bao kv get -format=json kv/${BAO_PREFIX:-garage}/keys/${GARAGE_KEY_NAME:-my-app-key} | jq -r '.data.data.state')"
  grant_state="$(bao kv get -format=json kv/${BAO_PREFIX:-garage}/grants/${GARAGE_GRANT_NAME:-${GARAGE_KEY_NAME:-my-app-key}--${GARAGE_BUCKET_NAME:-my-bucket}} | jq -r '.data.data.state')"
  access_key_id="$(bao kv get -format=json kv/${BAO_PREFIX:-garage}/keys/${GARAGE_KEY_NAME:-my-app-key} | jq -r '.data.data.access_key_id')"
  secret_access_key="$(bao kv get -format=json kv/${BAO_PREFIX:-garage}/keys/${GARAGE_KEY_NAME:-my-app-key} | jq -r '.data.data.secret_access_key')"

  if [ "${bucket_state}" != "ready" ]; then
    echo "FAIL ${label}: bucket state is '${bucket_state}' (expected 'ready'). State was not reconciled.${env_set}" >&2
    exit 1
  fi
  if [ "${key_state}" != "ready" ]; then
    echo "FAIL ${label}: key state is '${key_state}' (expected 'ready'). State was not reconciled.${env_set}" >&2
    exit 1
  fi
  if [ "${grant_state}" != "ready" ]; then
    echo "FAIL ${label}: grant state is '${grant_state}' (expected 'ready'). State was not reconciled.${env_set}" >&2
    exit 1
  fi
  if [ -z "${access_key_id}" ]; then
    echo "FAIL ${label}: access_key_id is empty. Key was not created.${env_set}" >&2
    exit 1
  fi
  if [ -z "${secret_access_key}" ]; then
    echo "FAIL ${label}: secret_access_key is empty. Key was not created.${env_set}" >&2
    exit 1
  fi

  echo "PASS ${label} smoke test"
  echo "  access_key_id: ${access_key_id}"
  echo "  secret_access_key: ***masked***"
}

test_ns "_"

for ns in test; do
  export BAO_NAMESPACE="${ns}"
  test_ns "${ns}"
done

echo "All smoke tests passed"
