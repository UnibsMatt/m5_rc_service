import { ago } from "../format.js";
import { Flag, Meter, OnlineBadge, flagsFor } from "./Status.jsx";

// Container/virtual bridges are rarely the address you want to reach the gateway on.
const VIRTUAL = /^(docker|br-|veth|virbr|cni|flannel|cali|kube)/;

function primaryIp(ips) {
  const v4 = ips.filter((i) => !i.ip.includes(":"));
  return (v4.find((i) => !VIRTUAL.test(i.iface)) ?? v4[0] ?? ips[0])?.ip;
}

export default function GatewayCard({ g, onOpen, selected }) {
  const ip = primaryIp(g.ips);
  const flags = flagsFor(g);
  return (
    <button
      type="button"
      onClick={() => onOpen(g.gateway_id)}
      className={`group flex w-full flex-col gap-3 rounded-xl border bg-surface p-4 text-left transition
        hover:border-series focus-visible:outline-2 focus-visible:outline-series
        ${selected ? "border-series" : "border-line"} ${g.online ? "" : "opacity-75"}`}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="truncate font-semibold text-ink">{g.gateway_id}</div>
          <div className="truncate text-xs text-ink-3">
            {g.hostname}{ip ? ` · ${ip}` : ""}
          </div>
        </div>
        <OnlineBadge online={g.online} />
      </div>

      <div className="flex items-end justify-between">
        <div>
          <div className="tabular text-3xl font-semibold leading-none text-ink">{g.iss_devices ?? "–"}</div>
          <div className="mt-1 text-xs text-ink-2">ISS devices</div>
        </div>
        <div className="text-right text-xs text-ink-3">seen {ago(g.seconds_since_seen)}</div>
      </div>

      <div className="grid gap-2">
        <Meter label="CPU" value={g.cpu_pct} />
        <Meter label="Memory" value={g.mem_pct} />
        <Meter label="Disk /" value={g.disk_pct} />
      </div>

      {flags.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {flags.map((f) => <Flag key={f.text} level={f.level}>{f.text}</Flag>)}
        </div>
      )}
    </button>
  );
}
