#!/usr/bin/env bash
set -euo pipefail

export BAO_ADDR="${BAO_ADDR:-http://127.0.0.1:8200}"
export BAO_TOKEN="${BAO_TOKEN:-root}"

echo "Waiting for OpenBao at ${BAO_ADDR}"
until bao status >/dev/null 2>&1; do
  sleep 1
done

if bao secrets list -format=json | jq -e '."kv/"' >/dev/null; then
  echo "KV mount kv/ already enabled"
else
  bao secrets enable -path=kv kv-v2
fi

cat <<'EOF' | bao policy write garage-controller - >/dev/null
path "kv/data/garage/*" {
  capabilities = ["create", "read", "update", "patch", "list"]
}

path "kv/metadata/garage/*" {
  capabilities = ["list", "read"]
}
EOF

echo "OpenBao initialization done"
