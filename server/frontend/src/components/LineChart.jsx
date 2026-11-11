import { useEffect, useMemo, useRef, useState } from "react";
import { time, dateTime } from "../format.js";

const H = 132;
const M = { top: 10, right: 12, bottom: 22, left: 40 };

/** Single-series line chart with crosshair + tooltip. Gaps (offline periods) break the line. */
export default function LineChart({ title, points, valueKey, max, format, bucketSecs }) {
  const wrap = useRef(null);
  const [width, setWidth] = useState(320);
  const [hover, setHover] = useState(null);

  useEffect(() => {
    const ro = new ResizeObserver(([e]) => setWidth(Math.max(200, e.contentRect.width)));
    if (wrap.current) ro.observe(wrap.current);
    return () => ro.disconnect();
  }, []);

  const data = useMemo(
    () => points.filter((p) => p[valueKey] != null).map((p) => ({ t: p.t, v: p[valueKey] })),
    [points, valueKey],
  );

  const w = width - M.left - M.right;
  const h = H - M.top - M.bottom;
  const t0 = data.length ? data[0].t : 0;
  const t1 = data.length ? data[data.length - 1].t : 1;
  const yMax = max ?? niceMax(Math.max(1, ...data.map((d) => d.v)));
  const x = (t) => M.left + (t1 === t0 ? w / 2 : ((t - t0) / (t1 - t0)) * w);
  const y = (v) => M.top + h - (v / yMax) * h;

  // Break the line where the gap is much larger than the sampling bucket.
  const gap = Math.max(bucketSecs * 3, 90);
  let d = "";
  data.forEach((p, i) => {
    const cmd = i === 0 || p.t - data[i - 1].t > gap ? "M" : "L";
    d += `${cmd}${x(p.t).toFixed(1)},${y(p.v).toFixed(1)}`;
  });

  function onMove(e) {
    if (!data.length) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const px = e.clientX - rect.left;
    let best = data[0];
    for (const p of data) if (Math.abs(x(p.t) - px) < Math.abs(x(best.t) - px)) best = p;
    setHover(best);
  }

  const latest = data.length ? data[data.length - 1].v : null;
  const ticks = [0, yMax / 2, yMax];

  return (
    <figure className="rounded-xl border border-line bg-surface p-3">
      <figcaption className="mb-1 flex items-baseline justify-between gap-2">
        <span className="text-sm font-medium text-ink">{title}</span>
        <span className="tabular text-sm text-ink-2">{latest == null ? "–" : format(latest)}</span>
      </figcaption>
      <div ref={wrap} className="relative">
        {data.length === 0 ? (
          <div className="flex h-[132px] items-center justify-center text-sm text-ink-3">
            No data in this range
          </div>
        ) : (
          <svg width={width} height={H} role="img" aria-label={`${title} over time`}
               onPointerMove={onMove} onPointerLeave={() => setHover(null)} className="block touch-none">
            {ticks.map((v) => (
              <g key={v}>
                <line x1={M.left} x2={width - M.right} y1={y(v)} y2={y(v)}
                      className="stroke-grid" strokeWidth="1" />
                <text x={M.left - 6} y={y(v)} dy="0.32em" textAnchor="end"
                      className="fill-ink-3 tabular text-[10px]">{format(v, true)}</text>
              </g>
            ))}
            <text x={M.left} y={H - 6} className="fill-ink-3 text-[10px]">{time(t0)}</text>
            <text x={width - M.right} y={H - 6} textAnchor="end" className="fill-ink-3 text-[10px]">{time(t1)}</text>

            <path d={d} fill="none" className="stroke-series" strokeWidth="2"
                  strokeLinejoin="round" strokeLinecap="round" />
            {data.length === 1 && <circle cx={x(data[0].t)} cy={y(data[0].v)} r="4" className="fill-series" />}

            {hover && (
              <g pointerEvents="none">
                <line x1={x(hover.t)} x2={x(hover.t)} y1={M.top} y2={M.top + h}
                      className="stroke-ink-3" strokeWidth="1" strokeDasharray="3 3" />
                <circle cx={x(hover.t)} cy={y(hover.v)} r="5" className="fill-series stroke-surface" strokeWidth="2" />
              </g>
            )}
            {/* full-size hit target */}
            <rect x={M.left} y={0} width={w} height={H} fill="transparent" />
          </svg>
        )}
        {hover && (
          <div className="pointer-events-none absolute top-0 z-10 rounded-md border border-line bg-raised px-2 py-1 text-xs shadow-sm"
               style={{ left: Math.min(Math.max(x(hover.t) - 60, 0), width - 130) }}>
            <div className="text-ink-3">{dateTime(hover.t)}</div>
            <div className="tabular font-medium text-ink">{format(hover.v)}</div>
          </div>
        )}
      </div>
    </figure>
  );
}

function niceMax(v) {
  const p = 10 ** Math.floor(Math.log10(v));
  for (const m of [1, 2, 5, 10]) if (v <= m * p) return m * p;
  return 10 * p;
}
