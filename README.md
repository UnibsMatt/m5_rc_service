# gw-monitor

Small Rust service for Alpine gateways. Every `INTERVAL_SECS` it collects

- CPU %, memory, disk usage, load, uptime, IP addresses
- the number of distinct ISS devices (MACs) seen on MQTT `/ISS/<MAC>/data|debug`

and sends one JSON report through the enabled **connectors**: `log`, `mqtt`, `api` (HTTPS + mutual TLS).

```
m5/
├── gw-monitor/            Rust crate (binary ~1.8 MB, static-ish musl, no OpenSSL)
│   └── src/
│       ├── main.rs        config → collect → fan-out loop
│       ├── config.rs      env vars (see conf.d)
│       ├── metrics.rs     /proc + statvfs + interfaces
│       ├── iss.rs         MQTT subscriber, MAC extraction + dedup
│       ├── cert.rs        client cert CN / expiry (gateway_id, warnings)
│       └── connectors/    log, mqtt, api (own thread, queue, retry, cert hot-reload)
├── server/                receiving side for the api connector: FastAPI + Postgres + React dashboard
│                          (docker compose; see server/README.md)
└── apk/                   abuild packaging
    ├── APKBUILD
    ├── build.sh           one-shot build → ~/packages/m5/<arch>/gw-monitor-<ver>-r<rel>.apk
    ├── gw-monitor.initd   OpenRC (supervise-daemon, runs as user gwmon)
    ├── gw-monitor.confd   all environment variables
    ├── gw-monitor.logrotate
    └── gw-monitor.{pre-install,post-install,post-upgrade,pre-deinstall}
```

## Build

```sh
apk add alpine-sdk cargo cargo-auditable     # once
/root/m5/apk/build.sh                        # runs unit tests, builds, signs
```
The package is signed with `~/.abuild/*.rsa`. Copy the matching `.rsa.pub` to `/etc/apk/keys/` on each
gateway (or install with `--allow-untrusted`). Build on the gateway's architecture (`apk --print-arch`);
for arm, build inside an `alpine:3.24` container of that platform.

## Install / upgrade / remove

```sh
apk add ./gw-monitor-0.1.0-r0.apk   # creates user gwmon, rc-update add default, starts it
apk add -u ./gw-monitor-0.1.1-r0.apk  # upgrade: restarts the service, keeps /etc/conf.d edits
apk del gw-monitor                  # stops and disables the service
```

## Configure

Edit `/etc/conf.d/gw-monitor`, then `rc-service gw-monitor restart`. Key settings:

| Variable | Default | |
|---|---|---|
| `CONNECTORS` | `log,mqtt` | any of `log`, `mqtt`, `api` |
| `INTERVAL_SECS` | `30` | report period |
| `MQTT_HOST` / `MQTT_PORT` | `127.0.0.1` / `31883` | |
| `MQTT_USER` / `MQTT_PASS` | `iss` / `iss123.` | |
| `ISS_TOPICS` | `/ISS/+/data,/ISS/+/debug` | the `+` level is the MAC; case-sensitive |
| `DEVICE_TTL_SECS` | `0` | 0 = all MACs since start, N = seen in last N s |
| `PUBLISH_TOPIC` | `gateway/status` | mqtt connector, retained, QoS 1 |
| `API_URL` | *(empty)* | api connector target (POST JSON) |
| `API_CLIENT_CERT` / `API_CLIENT_KEY` | `/dev/shm/client.crt` / `.key` | mTLS identity, re-read on change |
| `API_CA_FILE` | *(empty)* | CA that signed the API server cert (iotplt Root CA) |
| `GATEWAY_ID` | *(empty)* | default: client cert CN, else hostname |
| `IFACE` / `DISK_MOUNTS` | all / `/` | comma-separated filters |

### Enabling the API connector (when the server exists)

1. Test mTLS by hand:
   `curl -v --cert /dev/shm/client.crt --key /dev/shm/client.key --cacert /path/iotplt-root-ca.crt -H 'Content-Type: application/json' -d '{"ping":1}' https://<host>/<path>`
2. In conf.d: `API_URL=...`, `API_CA_FILE=/path/iotplt-root-ca.crt`, `CONNECTORS="log,mqtt,api"`
3. `rc-service gw-monitor restart`

API behaviour: 2xx = delivered; 4xx (except 408/429) = dropped and logged; network/TLS/5xx = retried
(`API_RETRIES`, backoff) then kept in a RAM queue (`API_QUEUE_MAX`, oldest dropped) and delivered in order
when the API is reachable again. Missing cert files → connector waits; a rotated cert is picked up without restart.

## Verify

```sh
rc-service gw-monitor status
tail -f /var/log/gw-monitor.log
mosquitto_sub -h 127.0.0.1 -p 31883 -u iss -P 'iss123.' -t gateway/status -C 1
```

Report example:
```json
{"schema":1,"ts":1794427771,"gateway_id":"pc-94px994","hostname":"iot-test-94px","version":"0.1.0",
 "uptime_s":4269292,"load":[0.92,0.65,0.36],"cpu_pct":2.7,
 "mem":{"total_kb":16063368,"available_kb":10529360,"used_kb":5534008,"pct":34.5},
 "disks":[{"mount":"/","total_kb":486489272,"avail_kb":420700304,"used_kb":41003204,"pct":8.9}],
 "ips":[{"iface":"br0","ip":"192.168.1.88"}],"iss_devices":2,"mqtt_connected":true,
 "client_cert":{"cn":"pc-94px994","not_after":1801835011,"days_left":85}}
```

## Notes

- Only one instance per MQTT client ID: two processes with the same `MQTT_CLIENT_ID` kick each other off the broker.
- `/dev/shm/client.key` is currently mode `0777`; the service warns about it. Fix in the provisioning
  (e.g. `root:iotcerts 0640`, add `gwmon` to `iotcerts`).
- `/dev/shm` is RAM: cert files are gone after reboot until re-provisioned; the api connector waits meanwhile.
- The client cert expires 2027-02-05; the service warns 14 days before.
