"""gw-fleet API: ingest for the gw-monitor `api` connector + read API for the dashboard.

Ingest arrives through nginx on the mTLS port. nginx passes the client-certificate result
in X-Client-Verify / X-Client-CN; this service is only reachable on the internal network,
and the dashboard proxy blanks those headers, so they can't be spoofed from outside.
"""

import asyncio
import logging
import os
from contextlib import asynccontextmanager
from datetime import datetime, timezone

from fastapi import FastAPI, Header, HTTPException, Query
from psycopg.rows import dict_row
from psycopg.types.json import Jsonb
from psycopg_pool import AsyncConnectionPool
from pydantic import BaseModel, ConfigDict, Field

log = logging.getLogger("gw-fleet")
logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")

DATABASE_URL = os.environ.get("DATABASE_URL", "postgresql://gwfleet:gwfleet@db:5432/gwfleet")
ONLINE_AFTER_SECS = int(os.environ.get("ONLINE_AFTER_SECS", "90"))
RETENTION_DAYS = int(os.environ.get("RETENTION_DAYS", "14"))
# verified = cert must chain to the CA in nginx's client-ca.crt
# present  = any client cert accepted (e.g. real iotplt certs before the iotplt CA is installed)
# off      = no client cert needed (plain testing only)
CLIENT_CERT_POLICY = os.environ.get("CLIENT_CERT_POLICY", "verified").lower()
# off | warn | enforce: report.gateway_id must equal the client cert CN
CN_MATCH = os.environ.get("CN_MATCH", "warn").lower()

SCHEMA = """
CREATE TABLE IF NOT EXISTS gateways (
    gateway_id    text PRIMARY KEY,
    hostname      text,
    version       text,
    client_cn     text,
    client_verify text,
    first_seen    timestamptz NOT NULL DEFAULT now(),
    last_seen     timestamptz NOT NULL DEFAULT now(),
    last_ts       bigint      NOT NULL,
    last_report   jsonb       NOT NULL
);
CREATE TABLE IF NOT EXISTS reports (
    id           bigserial PRIMARY KEY,
    gateway_id   text NOT NULL REFERENCES gateways ON DELETE CASCADE,
    ts           timestamptz NOT NULL,
    received_at  timestamptz NOT NULL DEFAULT now(),
    cpu_pct      real,
    mem_pct      real,
    disk_pct     real,
    iss_devices  integer,
    payload      jsonb NOT NULL,
    UNIQUE (gateway_id, ts)
);
CREATE INDEX IF NOT EXISTS reports_gateway_ts ON reports (gateway_id, ts DESC);
"""

pool = AsyncConnectionPool(DATABASE_URL, open=False, min_size=1, max_size=5,
                           kwargs={"row_factory": dict_row, "autocommit": True})


async def retention_loop() -> None:
    while True:
        try:
            async with pool.connection() as conn:
                cur = await conn.execute(
                    "DELETE FROM reports WHERE ts < now() - make_interval(days => %s)", (RETENTION_DAYS,))
                if cur.rowcount:
                    log.info("retention: deleted %s old reports", cur.rowcount)
        except Exception as e:  # keep the loop alive
            log.warning("retention failed: %s", e)
        await asyncio.sleep(3600)


@asynccontextmanager
async def lifespan(_: FastAPI):
    await pool.open(wait=True, timeout=60)
    async with pool.connection() as conn:
        await conn.execute(SCHEMA)
    task = asyncio.create_task(retention_loop())
    log.info("ready: cert_policy=%s cn_match=%s online_after=%ss retention=%sd",
             CLIENT_CERT_POLICY, CN_MATCH, ONLINE_AFTER_SECS, RETENTION_DAYS)
    yield
    task.cancel()
    await pool.close()


app = FastAPI(title="gw-fleet", version="0.1.0", lifespan=lifespan,
              docs_url="/api/docs", openapi_url="/api/openapi.json")


# ---------------------------------------------------------------- ingest -------

class Report(BaseModel):
    """gw-monitor report (schema 1). Unknown fields are kept in the stored payload."""
    model_config = ConfigDict(extra="allow", populate_by_name=True)

    schema_: int = Field(alias="schema", ge=1)
    ts: int = Field(gt=0)
    gateway_id: str = Field(min_length=1, max_length=128)
    hostname: str | None = None
    version: str | None = None
    cpu_pct: float | None = None
    mem: dict | None = None
    disks: list[dict] = []
    iss_devices: int | None = None


def check_client(verify: str, cn: str, gateway_id: str) -> None:
    if CLIENT_CERT_POLICY == "verified" and verify != "SUCCESS":
        raise HTTPException(401, f"client certificate not verified ({verify or 'NONE'})")
    if CLIENT_CERT_POLICY == "present" and (verify in ("", "NONE") or not cn):
        raise HTTPException(401, "client certificate required")
    if cn and cn != gateway_id and CN_MATCH != "off":
        msg = f"gateway_id {gateway_id!r} does not match client cert CN {cn!r}"
        if CN_MATCH == "enforce":
            raise HTTPException(403, msg)
        log.warning(msg)


@app.post("/api/v1/reports", status_code=201)
@app.put("/api/v1/reports", status_code=201)
async def ingest(report: Report,
                 x_client_verify: str = Header(default=""),
                 x_client_cn: str = Header(default="")):
    check_client(x_client_verify, x_client_cn, report.gateway_id)
    payload = report.model_dump(by_alias=True)
    disk_pct = next((d.get("pct") for d in report.disks if d.get("mount") == "/"),
                    report.disks[0].get("pct") if report.disks else None)
    ts = datetime.fromtimestamp(report.ts, tz=timezone.utc)

    async with pool.connection() as conn, conn.transaction():
        # Queued reports may arrive late/out of order: only a newer ts replaces last_report.
        await conn.execute(
            """
            INSERT INTO gateways (gateway_id, hostname, version, client_cn, client_verify,
                                  last_ts, last_report)
            VALUES (%(id)s, %(host)s, %(ver)s, %(cn)s, %(verify)s, %(ts)s, %(p)s)
            ON CONFLICT (gateway_id) DO UPDATE SET
                last_seen = now(),
                client_cn = EXCLUDED.client_cn,
                client_verify = EXCLUDED.client_verify,
                hostname    = CASE WHEN EXCLUDED.last_ts >= gateways.last_ts THEN EXCLUDED.hostname    ELSE gateways.hostname END,
                version     = CASE WHEN EXCLUDED.last_ts >= gateways.last_ts THEN EXCLUDED.version     ELSE gateways.version END,
                last_report = CASE WHEN EXCLUDED.last_ts >= gateways.last_ts THEN EXCLUDED.last_report ELSE gateways.last_report END,
                last_ts     = GREATEST(EXCLUDED.last_ts, gateways.last_ts)
            """,
            {"id": report.gateway_id, "host": report.hostname, "ver": report.version,
             "cn": x_client_cn or None, "verify": x_client_verify or None,
             "ts": report.ts, "p": Jsonb(payload)},
        )
        # (gateway_id, ts) is unique: a retried report is stored once.
        await conn.execute(
            """
            INSERT INTO reports (gateway_id, ts, cpu_pct, mem_pct, disk_pct, iss_devices, payload)
            VALUES (%s, %s, %s, %s, %s, %s, %s)
            ON CONFLICT (gateway_id, ts) DO NOTHING
            """,
            (report.gateway_id, ts, report.cpu_pct, (report.mem or {}).get("pct"), disk_pct,
             report.iss_devices, Jsonb(payload)),
        )
    return {"ok": True}


# ------------------------------------------------------------ dashboard --------

def summarize(row: dict) -> dict:
    r = row["last_report"]
    age = (datetime.now(timezone.utc) - row["last_seen"]).total_seconds()
    disks = r.get("disks") or []
    root = next((d for d in disks if d.get("mount") == "/"), disks[0] if disks else None)
    cert = r.get("client_cert") or {}
    return {
        "gateway_id": row["gateway_id"],
        "hostname": row["hostname"],
        "version": row["version"],
        "online": age <= ONLINE_AFTER_SECS,
        "last_seen": row["last_seen"].isoformat(),
        "seconds_since_seen": int(age),
        "report_ts": r.get("ts"),
        "cpu_pct": r.get("cpu_pct"),
        "mem_pct": (r.get("mem") or {}).get("pct"),
        "disk_pct": root.get("pct") if root else None,
        "iss_devices": r.get("iss_devices"),
        "mqtt_connected": r.get("mqtt_connected"),
        "ips": r.get("ips") or [],
        "client_cn": row["client_cn"],
        "client_verify": row["client_verify"],
        "cert_days_left": cert.get("days_left"),
    }


@app.get("/api/v1/gateways")
async def list_gateways():
    async with pool.connection() as conn:
        rows = await (await conn.execute("SELECT * FROM gateways ORDER BY gateway_id")).fetchall()
    return {"online_after_secs": ONLINE_AFTER_SECS, "gateways": [summarize(r) for r in rows]}


@app.get("/api/v1/gateways/{gateway_id}")
async def get_gateway(gateway_id: str):
    async with pool.connection() as conn:
        row = await (await conn.execute(
            "SELECT * FROM gateways WHERE gateway_id = %s", (gateway_id,))).fetchone()
        if not row:
            raise HTTPException(404, "unknown gateway")
        count = await (await conn.execute(
            "SELECT count(*) AS n FROM reports WHERE gateway_id = %s", (gateway_id,))).fetchone()
    return {**summarize(row), "first_seen": row["first_seen"].isoformat(),
            "stored_reports": count["n"], "report": row["last_report"]}


@app.get("/api/v1/gateways/{gateway_id}/history")
async def history(gateway_id: str, minutes: int = Query(60, ge=5, le=60 * 24 * 31)):
    # Average into at most ~240 buckets so long ranges stay light.
    bucket = max(1, minutes * 60 // 240)
    async with pool.connection() as conn:
        rows = await (await conn.execute(
            """
            SELECT extract(epoch FROM date_bin(make_interval(secs => %(b)s), ts, 'epoch'))::bigint AS t,
                   round(avg(cpu_pct)::numeric, 1)::float  AS cpu_pct,
                   round(avg(mem_pct)::numeric, 1)::float  AS mem_pct,
                   round(avg(disk_pct)::numeric, 1)::float AS disk_pct,
                   max(iss_devices)                        AS iss_devices
            FROM reports
            WHERE gateway_id = %(id)s AND ts > now() - make_interval(mins => %(m)s)
            GROUP BY 1 ORDER BY 1
            """, {"b": bucket, "id": gateway_id, "m": minutes})).fetchall()
    return {"gateway_id": gateway_id, "minutes": minutes, "bucket_secs": bucket, "points": rows}


@app.get("/healthz")
async def healthz():
    async with pool.connection() as conn:
        await conn.execute("SELECT 1")
    return {"ok": True}
