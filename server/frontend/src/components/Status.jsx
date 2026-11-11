// Status is never color-only: every badge has an icon and a text label.

export function OnlineBadge({ online }) {
  return online ? (
    <span className="inline-flex items-center gap-1.5 text-xs font-medium text-ink-2">
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
        <circle cx="5" cy="5" r="4" className="fill-good" />
      </svg>
      Online
    </span>
  ) : (
    <span className="inline-flex items-center gap-1.5 text-xs font-medium text-ink-2">
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
        <path d="M2 2 8 8M8 2 2 8" className="stroke-critical" strokeWidth="2" strokeLinecap="round" />
      </svg>
      Offline
    </span>
  );
}

export function Flag({ children, level = "warning" }) {
  const fill = level === "critical" ? "fill-critical" : "fill-warning";
  return (
    <span className="inline-flex items-center gap-1 rounded-full border border-line px-2 py-0.5 text-xs text-ink-2">
      <svg width="11" height="11" viewBox="0 0 12 12" aria-hidden="true">
        <path d="M6 1 11 10.5H1Z" className={fill} />
        <path d="M6 4.5v3M6 9v.01" className="stroke-ink" strokeWidth="1.3" strokeLinecap="round" />
      </svg>
      {children}
    </span>
  );
}

/** Warnings derived from a gateway summary. */
export function flagsFor(g) {
  const out = [];
  if (g.online && g.mqtt_connected === false) out.push({ text: "MQTT disconnected", level: "warning" });
  if (g.cert_days_left != null && g.cert_days_left < 0) out.push({ text: "Cert expired", level: "critical" });
  else if (g.cert_days_left != null && g.cert_days_left < 14)
    out.push({ text: `Cert expires in ${g.cert_days_left}d`, level: "warning" });
  if (g.client_cn && g.client_cn !== g.gateway_id) out.push({ text: "CN mismatch", level: "warning" });
  if (g.disk_pct != null && g.disk_pct >= 90) out.push({ text: "Disk almost full", level: "warning" });
  return out;
}

/** Thin usage meter for a percentage. */
export function Meter({ label, value }) {
  const v = value == null ? 0 : Math.min(100, Math.max(0, value));
  return (
    <div>
      <div className="mb-1 flex justify-between text-xs">
        <span className="text-ink-2">{label}</span>
        <span className="tabular text-ink">{value == null ? "–" : `${value.toFixed(1)}%`}</span>
      </div>
      <div className="h-1.5 rounded-full bg-grid" role="meter" aria-valuenow={v} aria-valuemin="0" aria-valuemax="100" aria-label={label}>
        <div className="h-1.5 rounded-full bg-series" style={{ width: `${v}%` }} />
      </div>
    </div>
  );
}
