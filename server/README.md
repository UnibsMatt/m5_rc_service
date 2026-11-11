# gw-fleet server

Receives reports from the gw-monitor `api` connector and shows every gateway on a dashboard.

```
gateway (gw-monitor) ──HTTPS + client cert──▶ :8443 ┐
                                                    │  web (nginx)  ──▶ api (FastAPI) ──▶ db (Postgres 17)
browser ──────────────HTTP───────────────────▶ :8090 ┘  React build      ingest + read API
```

| Service | Image | Size | Role |
|---|---|---|---|
| `web` | `gw-fleet-web` (node build → `nginx:1.30-alpine-slim`) | 13 MB | dashboard, read-only `/api` proxy, mTLS ingest |
| `api` | `gw-fleet-api` (venv build → `python:3.12-alpine`) | 79 MB | FastAPI, runs as non-root |
| `db` | `postgres:17-alpine` | stock | `gateways` (latest state) + `reports` (history) |

## Run

```sh
cd server
cp .env.example .env            # set POSTGRES_PASSWORD; pick CLIENT_CERT_POLICY
./certs/gen-dev-certs.sh IP:192.168.1.88 DNS:fleet.lan   # SANs the gateways will use
docker compose up -d --build
```
Dashboard: `http://<host>:8090` (click a gateway for details and history; `?gw=<id>` links straight to it).
API docs: `http://<host>:8090/api/docs`.

## Point a gateway at it

In `/etc/conf.d/gw-monitor` on the gateway, then `rc-service gw-monitor restart`:
```sh
export CONNECTORS="log,mqtt,api"
export API_URL="https://192.168.1.88:8443/api/v1/reports"
export API_CA_FILE="/etc/gw-monitor/fleet-ca.crt"   # copy of server/certs/ca.crt
# API_CLIENT_CERT / API_CLIENT_KEY stay /dev/shm/client.crt / client.key
```

## Client certificates

nginx asks every gateway for a client certificate (`optional_no_ca`), verifies it against
`certs/client-ca.crt`, and passes the result to the API, which applies `CLIENT_CERT_POLICY`:

| Policy | Accepts | Use |
|---|---|---|
| `verified` | certs chaining to `certs/client-ca.crt` | production |
| `present` | any client cert (verification result is stored and shown) | until the iotplt Root CA is available |
| `off` | anything | local testing only |

For production: append the **iotplt Root CA** to `certs/client-ca.crt`, set `CLIENT_CERT_POLICY=verified`,
then `docker compose up -d`. `CN_MATCH=enforce` additionally rejects reports whose `gateway_id`
differs from the cert CN (`warn` only logs and flags it on the dashboard).

`X-Client-*` headers sent by clients are always overwritten by nginx, and the dashboard port only
allows `GET`, so reports can only enter through the mTLS port.

## Behaviour

- **Online/offline**: offline after `ONLINE_AFTER_SECS` (default 90 = 3 × the 30 s report interval).
  Reports backfilled from a gateway's queue count at their own timestamp, so a recovered queue
  doesn't make a long-dead gateway look online.
- **Duplicates**: `(gateway_id, ts)` is unique, so a retried report is stored once.
- **Order**: late reports go to history; only a newer report replaces the gateway's current state.
- **Retention**: reports older than `RETENTION_DAYS` are deleted hourly.
- **History API** averages into ≤ 240 buckets, so the 7-day view stays light.

## API

| Method | Path | Port |
|---|---|---|
| `POST`/`PUT` | `/api/v1/reports` | 8443 (mTLS) |
| `GET` | `/api/v1/gateways` | 8090 |
| `GET` | `/api/v1/gateways/{id}` | 8090 |
| `GET` | `/api/v1/gateways/{id}/history?minutes=60` | 8090 |
| `GET` | `/healthz` | internal |

## Frontend development

```sh
cd frontend && npm ci && npm run dev    # Vite on :5173, proxies /api to the stack on :8090
```
