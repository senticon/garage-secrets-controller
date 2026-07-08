#!/usr/bin/env bash
set -euo pipefail

GARAGE_ADMIN_URL="${GARAGE_ADMIN_URL:-http://127.0.0.1:3903}"
GARAGE_ADMIN_TOKEN="${GARAGE_ADMIN_TOKEN:-dev-garage-admin-token}"
TIMEOUT_SECONDS="${TIMEOUT_SECONDS:-90}"
ZONE="${GARAGE_ZONE:-local}"
CAPACITY="${GARAGE_CAPACITY:-1G}"
SERVICE_NAME="${GARAGE_SERVICE_NAME:-garage}"
BUCKET_NAME="${GARAGE_BUCKET_NAME:-my-bucket}"
KEY_NAME="${GARAGE_KEY_NAME:-my-app-key}"
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

garage_status() {
  curl -fsS \
    -H "Authorization: Bearer ${GARAGE_ADMIN_TOKEN}" \
    "${GARAGE_ADMIN_URL}/v2/GetClusterHealth"
}

wait_admin() {
  local deadline
  deadline=$(( $(date +%s) + TIMEOUT_SECONDS ))
  until garage_status >/dev/null 2>&1; do
    if [ "$(date +%s)" -ge "$deadline" ]; then
      echo "Timed out waiting for Garage admin API" >&2
      exit 1
    fi
    sleep 1
  done
}

bootstrap_layout() {
  local node_id
  node_id="$(docker compose exec -T "${SERVICE_NAME}" /garage status 2>/dev/null | awk '/^==== HEALTHY NODES ====/{getline; getline; print $1}')"
  if [ -z "${node_id}" ]; then
    echo "Could not determine Garage node id from /garage status" >&2
    exit 1
  fi

  docker compose exec -T "${SERVICE_NAME}" /garage layout assign -z "${ZONE}" -c "${CAPACITY}" "${node_id}" || true
  docker compose exec -T "${SERVICE_NAME}" /garage layout apply --version 1 || true
}

wait_bucket_api() {
  local deadline
  deadline=$(( $(date +%s) + TIMEOUT_SECONDS ))
  until curl -fsS \
    -H "Authorization: Bearer ${GARAGE_ADMIN_TOKEN}" \
    "${GARAGE_ADMIN_URL}/health" >/dev/null 2>&1; do
    if [ "$(date +%s)" -ge "$deadline" ]; then
      echo "Garage bucket API is still unhealthy" >&2
      garage_status | jq . >&2 || true
      exit 1
    fi
    sleep 1
  done
}

recreate_resources() {
  echo "Cleaning up stale keys/buckets"

  local key_ids
  key_ids="$(docker compose exec -T "${SERVICE_NAME}" /garage key list 2>/dev/null | awk -v key_name="${KEY_NAME}" '$1 ~ /^GK/ && $3 == key_name { print $1 }')"
  if [ -n "${key_ids}" ]; then
    while IFS= read -r key_id; do
      [ -z "${key_id}" ] && continue
      echo "Removing key ${key_id} (${KEY_NAME})"
      docker compose exec -T "${SERVICE_NAME}" /garage key delete "${key_id}" --yes >/dev/null 2>&1 || \
        docker compose exec -T "${SERVICE_NAME}" /garage key rm "${key_id}" --yes >/dev/null 2>&1 || true
    done <<EOF
${key_ids}
EOF
  fi

  local bucket_id
  bucket_id="$(docker compose exec -T "${SERVICE_NAME}" /garage bucket list 2>/dev/null | awk -v bucket_name="${BUCKET_NAME}" '$1 ~ /^b/ && $2 == bucket_name { print $1; exit }')"
  if [ -n "${bucket_id}" ]; then
    echo "Removing bucket ${bucket_id} (${BUCKET_NAME})"
    docker compose exec -T "${SERVICE_NAME}" /garage bucket delete "${bucket_id}" --yes >/dev/null 2>&1 || \
      docker compose exec -T "${SERVICE_NAME}" /garage bucket rm "${bucket_id}" --yes >/dev/null 2>&1 || true
  fi
}

echo "Initializing Garage with defaults: zone=${ZONE}, capacity=${CAPACITY}, timeout=${TIMEOUT_SECONDS}s"
#wait_admin
bootstrap_layout
wait_bucket_api

# Always clean up stale keys/buckets before proceeding so that lookups return single
# results and the reconciler can create fresh resources cleanly.
recreate_resources

echo "Garage initialized and healthy"
garage_status | jq .
