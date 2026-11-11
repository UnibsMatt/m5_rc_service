import { useEffect, useState } from "react";
import { api } from "../api.js";
import { ago, dateTime, duration, kb, pct } from "../format.js";
import LineChart from "./LineChart.jsx";
import { Flag, OnlineBadge, flagsFor } from "./Status.jsx";

const RANGES = [
  { label: "1h", minutes: 60 },
  { label: "6h", minutes: 360 },
  { label: "24h", minutes: 1440 },
  { label: "7d", minutes: 10080 },
];

export default function DetailPanel({ id, onClose, refreshKey }) {
  const [detail, setDetail] = useState(null);
  const [hist, setHist] = useState(null);
  const [range, setRange] = useState(60);
  const [error, setError] = useState(null);
  const [showTable, setShowTable] = useState(false);

  useEffect(() => {
    let alive = true;
    Promise.all([api.gateway(id), api.history(id, range)])
      .then(([d, h]) => { if (alive) { setDetail(d); setHist(h); setError(null); } })
      .catch((e) => alive && setError(e.message));
    return () => { alive = false; };
  }, [id, range, refreshKey]);

  useEffect(() => {
    const onKey = (e) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const r = detail?.report;
  const points = hist?.points ?? [];

  return (
    <div className="fixed inset-0 z-40 flex justify-end" role="dialog" aria-modal="true" aria-label={`Gateway ${id}`}>
      <div className="absolute inset-0 bg-black/30" onClick={onClose} />
      <aside className="relative flex h-full w-full max-w-2xl flex-col overflow-y-auto border-l border-line bg-page shadow-xl">
        <header className="sticky top-0 z-10 flex items-start justify-between gap-4 border-b border-line bg-page/95 px-5 py-4 backdrop-blur">
          <div className="min-w-0">
            <h2 className="truncate text-lg font-semibold text-ink">{id}</h2>
            {detail && (
              <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-ink-3">
                <OnlineBadge online={detail.online} />
                <span>last report {ago(detail.seconds_since_seen)}</span>
                <span>v{detail.version}</span>
              </div>
            )}
          </div>
          <button type="button" onClick={onClose} aria-label="Close"
                  className="rounded-md p-1.5 text-ink-2 hover:bg-surface hover:text-ink">
            <svg width="18" height="18" viewBox="0 0 18 18" aria-hidden="true">
              <path d="M4 4l10 10M14 4 4 14" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
            </svg>
          </button>
        </header>

        {error && <p className="m-5 rounded-lg border border-line bg-surface p-3 text-sm text-ink-2">Could not load: {error}</p>}
        {!detail && !error && <p className="m-5 text-sm text-ink-3">Loading…</p>}

        {detail && r && (
          <div className="flex flex-col gap-6 px-5 py-5">
            {flagsFor(detail).length > 0 && (
              <div className="flex flex-wrap gap-1.5">
                {flagsFor(detail).map((f) => <Flag key={f.text} level={f.level}>{f.text}</Flag>)}
              </div>
            )}

            <section className="grid grid-cols-2 gap-3 sm:grid-cols-4">
              <Stat label="ISS devices" value={r.iss_devices ?? "–"} />
              <Stat label="CPU" value={pct(r.cpu_pct)} />
              <Stat label="Memory" value={pct(r.mem?.pct)} sub={r.mem && `${kb(r.mem.used_kb)} / ${kb(r.mem.total_kb)}`} />
              <Stat label="Uptime" value={duration(r.uptime_s)} sub={`load ${r.load?.map((l) => l.toFixed(2)).join(" ")}`} />
            </section>

            <section>
              <div className="mb-2 flex items-center justify-between gap-2">
                <h3 className="text-sm font-semibold text-ink">History</h3>
                <div className="flex items-center gap-2">
                  <div className="flex rounded-lg border border-line p-0.5" role="group" aria-label="Time range">
                    {RANGES.map((x) => (
                      <button key={x.minutes} type="button" onClick={() => setRange(x.minutes)}
                              aria-pressed={range === x.minutes}
                              className={`rounded-md px-2.5 py-1 text-xs ${range === x.minutes ? "bg-surface font-medium text-ink shadow-sm" : "text-ink-2 hover:text-ink"}`}>
                        {x.label}
                      </button>
                    ))}
                  </div>
                  <button type="button" onClick={() => setShowTable((s) => !s)}
                          className="rounded-lg border border-line px-2.5 py-1 text-xs text-ink-2 hover:text-ink">
                    {showTable ? "Charts" : "Table"}
                  </button>
                </div>
              </div>
              {showTable ? (
                <HistoryTable points={points} />
              ) : (
                <div className="grid gap-3">
                  <LineChart title="ISS devices" points={points} valueKey="iss_devices"
                             bucketSecs={hist?.bucket_secs ?? 60} format={(v) => `${Math.round(v)}`} />
                  <div className="grid gap-3 sm:grid-cols-2">
                    <LineChart title="CPU %" points={points} valueKey="cpu_pct" max={100}
                               bucketSecs={hist?.bucket_secs ?? 60} format={(v, axis) => (axis ? `${v}` : `${v.toFixed(1)}%`)} />
                    <LineChart title="Memory %" points={points} valueKey="mem_pct" max={100}
                               bucketSecs={hist?.bucket_secs ?? 60} format={(v, axis) => (axis ? `${v}` : `${v.toFixed(1)}%`)} />
                  </div>
                </div>
              )}
            </section>

            <Section title="Disks">
              <Table head={["Mount", "Used", "Free", "Total", "Use"]}
                     rows={(r.disks ?? []).map((d) => [d.mount, kb(d.used_kb), kb(d.avail_kb), kb(d.total_kb), pct(d.pct)])} />
            </Section>

            <Section title="Network">
              <Table head={["Interface", "Address"]} rows={(r.ips ?? []).map((i) => [i.iface, i.ip])} />
            </Section>

            <Section title="Identity & connectivity">
              <dl className="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-1.5 text-sm">
                <Row k="Gateway ID" v={detail.gateway_id} />
                <Row k="Hostname" v={detail.hostname} />
                <Row k="Client cert CN" v={detail.client_cn ?? "none"} />
                <Row k="Cert verification" v={detail.client_verify ?? "–"} />
                <Row k="Cert expires" v={r.client_cert?.not_after ? `${dateTime(r.client_cert.not_after)} (${r.client_cert.days_left} days)` : "–"} />
                <Row k="MQTT broker" v={r.mqtt_connected ? "connected" : "disconnected"} />
                <Row k="First seen" v={new Date(detail.first_seen).toLocaleString()} />
                <Row k="Report time" v={dateTime(r.ts)} />
                <Row k="Stored reports" v={detail.stored_reports} />
              </dl>
            </Section>

            <details className="rounded-xl border border-line bg-surface">
              <summary className="cursor-pointer px-4 py-2.5 text-sm font-medium text-ink">Raw report</summary>
              <pre className="overflow-x-auto border-t border-line p-4 font-mono text-xs text-ink-2">
                {JSON.stringify(r, null, 2)}
              </pre>
            </details>
          </div>
        )}
      </aside>
    </div>
  );
}

function Stat({ label, value, sub }) {
  return (
    <div className="rounded-xl border border-line bg-surface p-3">
      <div className="text-xs text-ink-2">{label}</div>
      <div className="tabular mt-1 text-xl font-semibold text-ink">{value}</div>
      {sub && <div className="tabular mt-0.5 truncate text-xs text-ink-3">{sub}</div>}
    </div>
  );
}

function Section({ title, children }) {
  return (
    <section>
      <h3 className="mb-2 text-sm font-semibold text-ink">{title}</h3>
      {children}
    </section>
  );
}

function Row({ k, v }) {
  return (
    <>
      <dt className="text-ink-3">{k}</dt>
      <dd className="min-w-0 break-words text-ink">{v}</dd>
    </>
  );
}

function Table({ head, rows }) {
  if (!rows.length) return <p className="text-sm text-ink-3">None reported.</p>;
  return (
    <div className="overflow-x-auto rounded-xl border border-line bg-surface">
      <table className="w-full text-sm">
        <thead>
          <tr className="border-b border-line text-left text-xs text-ink-3">
            {head.map((h) => <th key={h} className="px-3 py-2 font-medium">{h}</th>)}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, i) => (
            <tr key={i} className="border-b border-line last:border-0">
              {row.map((c, j) => <td key={j} className="tabular px-3 py-1.5 text-ink">{c}</td>)}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function HistoryTable({ points }) {
  const rows = [...points].reverse().slice(0, 200).map((p) => [
    dateTime(p.t), p.iss_devices ?? "–", pct(p.cpu_pct), pct(p.mem_pct), pct(p.disk_pct),
  ]);
  return <Table head={["Time", "ISS devices", "CPU", "Memory", "Disk /"]} rows={rows} />;
}
