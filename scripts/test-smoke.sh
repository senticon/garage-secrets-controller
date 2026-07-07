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

#printf "Waiting for Garage"
#until curl -fsS -H "Authorization: Bearer ${GARAGE_ADMIN_TOKEN}" "${GARAGE_ADMIN_URL}/health" >/dev/null 2>&1; do
#  printf "."
#  sleep 1
#done
#printf "\n"

./scripts/init-openbao.sh
if [ "${RECREATE}" = "true" ]; then
  ./scripts/init-garage.sh --recreate
  ./scripts/seed-openbao.sh --recreate
else
  ./scripts/init-garage.sh
  ./scripts/seed-openbao.sh
fi

docker compose run --rm controller --once

bucket_state="$(bao kv get -format=json kv/garage/buckets/my-bucket | jq -r '.data.data.state')"
key_state="$(bao kv get -format=json kv/garage/keys/my-app-key | jq -r '.data.data.state')"
grant_state="$(bao kv get -format=json kv/garage/grants/my-app-key--my-bucket | jq -r '.data.data.state')"
access_key_id="$(bao kv get -format=json kv/garage/keys/my-app-key | jq -r '.data.data.access_key_id')"
secret_access_key="$(bao kv get -format=json kv/garage/keys/my-app-key | jq -r '.data.data.secret_access_key')"

test "${bucket_state}" = "ready"
test "${key_state}" = "ready"
test "${grant_state}" = "ready"
test -n "${access_key_id}"
test -n "${secret_access_key}"

echo "Smoke test passed"
echo "Generated key id: ${access_key_id}"
echo "Generated secret: ***masked***"
