#!/usr/bin/env bash
set -euo pipefail

export BAO_ADDR="${BAO_ADDR:-http://127.0.0.1:8200}"
export BAO_TOKEN="${BAO_TOKEN:-root}"
export BAO_NAMESPACE="${BAO_NAMESPACE:-}"

echo "Waiting for OpenBao at ${BAO_ADDR}"
until bao status >/dev/null 2>&1; do
  sleep 1
done

# Helper: check if kv mount exists for a given namespace
kv_mounted_at() {
  local ns="$1"
  if [ -z "${ns}" ]; then
    bao secrets list -format=json 2>/dev/null | jq -e '."kv/"' >/dev/null
  else
    bao secrets list -format=json -namespace="${ns}" 2>/dev/null | jq -e '."kv/"' >/dev/null
  fi
}

# Enable KV at root if not present, unless a specific namespace is configured
roots_append="false"
if [ -z "${BAO_NAMESPACE}" ]; then
  if kv_mounted_at ""; then
    echo "KV mount kv/ already enabled at root"
  else
    echo "Enabling KV v2 at kv/"
    bao secrets enable -path=kv kv-v2
  fi
else
  roots_append="true"
fi

# Enable KV in default and test namespaces
for ns in default test; do
  if kv_mounted_at "${ns}"; then
    echo "KV mount kv/ already enabled in ${ns} namespace"
  else
    echo "Enabling KV v2 in ${ns} namespace"
    bao secrets enable -path=kv -namespace="${ns}" kv-v2
  fi
done

if [ "${roots_append}" = "true" ] && ! kv_mounted_at ""; then
  echo "Enabling KV v2 at kv/ (auto-mode)"
  bao secrets enable -path=kv kv-v2
fi

if [ -z "${BAO_NAMESPACE}" ]; then
  cat <<'EOF' | bao policy write garage-controller - >/dev/null
path "kv/data/garage/*" {
  capabilities = ["create", "read", "update", "patch", "list"]
}

path "kv/metadata/garage/*" {
  capabilities = ["list", "read"]
}
EOF
else
  cat <<EOF | bao policy write garage-controller -namespace="${BAO_NAMESPACE}" - >/dev/null
path "kv/data/garage/*" {
  capabilities = ["create", "read", "update", "patch", "list"]
}

path "kv/metadata/garage/*" {
  capabilities = ["list", "read"]
}
EOF
fi

echo "OpenBao initialization done"
