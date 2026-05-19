# Development

Use the local Compose stack for controller development and smoke testing. It starts OpenBao in dev mode, a single-node
Garage instance, and the controller.

## Security Warning

The provided Compose stack is for local development only:

- OpenBao runs in dev mode with root token `root`.
- Garage uses a fixed static admin token.
- The bootstrap flow and default ports assume a trusted local machine.

Do not use this stack or these defaults in production.

## Local Stack

Prerequisites:

- Docker and Docker Compose
- `bao` CLI
- `jq`

Start Garage and OpenBao, then initialize local state:

```bash
docker compose up -d --build openbao garage
./scripts/init-garage.sh
./scripts/init-openbao.sh
./scripts/seed-openbao.sh
docker compose run --rm controller --once
bao kv get kv/garage/keys/my-app-key
```

Single-node Garage needs a layout bootstrap before bucket and key APIs become healthy. `./scripts/init-garage.sh`
handles the `assign` and `apply` calls, then waits for health.

`./scripts/init-garage.sh` defaults:

- Service: `garage`
- Zone: `local`
- Capacity: `1G`
- Timeout: `90s`

Use `--recreate` with `./scripts/init-garage.sh` and `./scripts/seed-openbao.sh` to remove existing test resources
before reseeding.

Run the full stack, including the controller in continuous mode:

```bash
docker compose up -d --build
```

## Smoke Test

Run the end-to-end smoke path:

```bash
./scripts/test-smoke.sh
```

Clean rerun against existing dev data:

```bash
RECREATE=true ./scripts/test-smoke.sh
```

## Local Files

- `compose.yaml`: OpenBao, Garage, controller, and toolbox services.
- `config/garage.toml`: single-node Garage config.
- `scripts/init-openbao.sh`: idempotent KV v2 setup.
- `scripts/seed-openbao.sh`: seed requested records.
- `scripts/test-smoke.sh`: end-to-end smoke path.
- `scripts/init-garage.sh`: idempotent Garage single-node init.
- `scripts/garage-status.sh`: admin status helper.

## Project Notes

V1 is forward-only. It creates, adopts where safe, and reconciles forward. It does not rotate credentials or fully
correct permission drift.

Non-goals for the current version:

- Full drift correction with `DenyBucketKey`.
- Key rotation.
- Production-grade OpenBao auth in the local stack.
- Watch-based reconciliation.

Possible next work:

- CAS support for safer concurrent OpenBao updates.
- `DenyBucketKey`-based drift correction.
- Rotation workflow for S3 credentials.
- AppRole or another hardened OpenBao auth method.
- Prometheus metrics.
- Health endpoint.
