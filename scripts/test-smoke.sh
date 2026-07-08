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

docker compose run --rm controller --once

check_ns() {
  local ns="$1"
  local env_set=""
  if [ -n "${ns}" ] && [ "${ns}" != "_" ]; then
    export BAO_NAMESPACE="${ns}"
  fi

  local bucket_state key_state grant_state access_key_id secret_access_key
  bucket_state="$(bao kv get -format=json kv/${BAO_PREFIX:-garage}/buckets/${GARAGE_BUCKET_NAME:-my-bucket} | jq -r '.data.data.state')"
  key_state="$(bao kv get -format=json kv/${BAO_PREFIX:-garage}/keys/${GARAGE_KEY_NAME:-my-app-key} | jq -r '.data.data.state')"
  grant_state="$(bao kv get -format=json kv/${BAO_PREFIX:-garage}/grants/${GARAGE_GRANT_NAME:-${GARAGE_KEY_NAME:-my-app-key}--${GARAGE_BUCKET_NAME:-my-bucket}} | jq -r '.data.data.state')"
  access_key_id="$(bao kv get -format=json kv/${BAO_PREFIX:-garage}/keys/${GARAGE_KEY_NAME:-my-app-key} | jq -r '.data.data.access_key_id')"
  secret_access_key="$(bao kv get -format=json kv/${BAO_PREFIX:-garage}/keys/${GARAGE_KEY_NAME:-my-app-key} | jq -r '.data.data.secret_access_key')"

  test "${bucket_state}" = "ready"
  test "${key_state}" = "ready"
  test "${grant_state}" = "ready"
  test -n "${access_key_id}"
  test -n "${secret_access_key}"

  echo "Namespace '${ns:-root}' smoke test passed"
  echo "Generated key id: ${access_key_id}"
  echo "Generated secret: ***masked***"
}

check_ns "_"

for ns in default test; do
  export BAO_NAMESPACE="${ns}"
  check_ns "${ns}"
done

echo "All smoke tests passed"
