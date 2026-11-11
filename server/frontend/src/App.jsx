import { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "./api.js";
import DetailPanel from "./components/DetailPanel.jsx";
import GatewayCard from "./components/GatewayCard.jsx";

const REFRESH_MS = 10_000;

export default function App() {
  const [data, setData] = useState(null);
  const [error, setError] = useState(null);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState("all");
  const [selected, setSelected] = useState(() => new URLSearchParams(location.search).get("gw"));
  const [tick, setTick] = useState(0);

  useEffect(() => {
    let alive = true;
    api.gateways()
      .then((d) => { if (alive) { setData(d); setError(null); } })
      .catch((e) => alive && setError(e.message));
    const t = setTimeout(() => setTick((n) => n + 1), REFRESH_MS);
    return () => { alive = false; clearTimeout(t); };
  }, [tick]);

  // Keep the open gateway in the URL so a detail view can be shared/reloaded.
  const open = useCallback((id) => {
    setSelected(id);
    const u = new URL(location.href);
    id ? u.searchParams.set("gw", id) : u.searchParams.delete("gw");
    history.replaceState(null, "", u);
  }, []);
  const close = useCallback(() => open(null), [open]);

  const gateways = data?.gateways ?? [];
  const totals = useMemo(() => {
    const online = gateways.filter((g) => g.online);
    return {
      all: gateways.length,
      online: online.length,
      offline: gateways.length - online.length,
      devices: online.reduce((s, g) => s + (g.iss_devices ?? 0), 0),
    };
  }, [gateways]);

  const shown = gateways
    .filter((g) => filter === "all" || (filter === "online" ? g.online : !g.online))
    .filter((g) => {
      const q = query.trim().toLowerCase();
      return !q || [g.gateway_id, g.hostname, ...g.ips.map((i) => i.ip)].some((s) => s?.toLowerCase().includes(q));
    })
    .sort((a, b) => Number(b.online) - Number(a.online) || a.gateway_id.localeCompare(b.gateway_id));

  return (
    <div className="mx-auto max-w-7xl px-4 py-6 sm:px-6">
      <header className="mb-6 flex flex-wrap items-end justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold text-ink">Gateway Fleet</h1>
          <p className="text-sm text-ink-2">Gateways reporting through the gw-monitor API connector</p>
        </div>
        <p className="text-xs text-ink-3" aria-live="polite">
          {error ? `Update failed: ${error}` : data ? `Auto-refresh every ${REFRESH_MS / 1000}s` : "Loading…"}
        </p>
      </header>

      <section className="mb-6 grid grid-cols-2 gap-3 sm:grid-cols-4" aria-label="Fleet summary">
        <Tile label="Gateways" value={totals.all} />
        <Tile label="Online" value={totals.online} />
        <Tile label="Offline" value={totals.offline} />
        <Tile label="ISS devices (online gateways)" value={totals.devices} />
      </section>

      <div className="mb-4 flex flex-wrap items-center gap-3">
        <input
          type="search" value={query} onChange={(e) => setQuery(e.target.value)}
          placeholder="Search gateway, hostname or IP"
          className="min-w-0 flex-1 rounded-lg border border-line bg-surface px-3 py-2 text-sm text-ink placeholder:text-ink-3 focus:border-series focus:outline-none sm:max-w-sm"
        />
        <div className="flex rounded-lg border border-line p-0.5" role="group" aria-label="Status filter">
          {["all", "online", "offline"].map((f) => (
            <button key={f} type="button" onClick={() => setFilter(f)} aria-pressed={filter === f}
                    className={`rounded-md px-3 py-1.5 text-xs capitalize ${filter === f ? "bg-surface font-medium text-ink shadow-sm" : "text-ink-2 hover:text-ink"}`}>
              {f}
            </button>
          ))}
        </div>
      </div>

      {data && gateways.length === 0 && (
        <div className="rounded-xl border border-dashed border-line p-10 text-center">
          <p className="font-medium text-ink">No gateway has reported yet</p>
          <p className="mt-1 text-sm text-ink-2">
            On a gateway set <code className="font-mono text-xs">API_URL</code> to this server's
            ingest endpoint and add <code className="font-mono text-xs">api</code> to <code className="font-mono text-xs">CONNECTORS</code>.
          </p>
        </div>
      )}
      {data && gateways.length > 0 && shown.length === 0 && (
        <p className="py-10 text-center text-sm text-ink-2">No gateways match the filter.</p>
      )}

      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        {shown.map((g) => (
          <GatewayCard key={g.gateway_id} g={g} onOpen={open} selected={g.gateway_id === selected} />
        ))}
      </div>

      {selected && <DetailPanel id={selected} onClose={close} refreshKey={tick} />}
    </div>
  );
}

function Tile({ label, value }) {
  return (
    <div className="rounded-xl border border-line bg-surface p-4">
      <div className="tabular text-3xl font-semibold leading-none text-ink">{value}</div>
      <div className="mt-2 text-xs text-ink-2">{label}</div>
    </div>
  );
}
