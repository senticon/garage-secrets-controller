#!/usr/bin/env bash
set -euo pipefail

GARAGE_ADMIN_URL="${GARAGE_ADMIN_URL:-http://127.0.0.1:3903}"
GARAGE_ADMIN_TOKEN="${GARAGE_ADMIN_TOKEN:-dev-garage-admin-token}"
ENDPOINT="${GARAGE_ADMIN_URL}/v1/status"

>&2 echo "Garage status endpoint: ${ENDPOINT}"

curl -fsS \
  -H "Authorization: Bearer ${GARAGE_ADMIN_TOKEN}" \
  "${ENDPOINT}" | jq .
