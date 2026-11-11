async function get(path) {
  const r = await fetch(path, { headers: { Accept: "application/json" } });
  if (!r.ok) throw new Error(`${r.status} ${r.statusText}`);
  return r.json();
}

export const api = {
  gateways: () => get("/api/v1/gateways"),
  gateway: (id) => get(`/api/v1/gateways/${encodeURIComponent(id)}`),
  history: (id, minutes) =>
    get(`/api/v1/gateways/${encodeURIComponent(id)}/history?minutes=${minutes}`),
};
