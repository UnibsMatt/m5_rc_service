//! gw-monitor: gateway metrics (CPU, memory, disk, IP) + count of distinct ISS devices
//! seen on MQTT, delivered every INTERVAL_SECS through the configured connectors.

mod cert;
mod config;
mod connectors;
mod iss;
mod metrics;
mod util;

use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use config::Config;
use connectors::api::ApiConnector;
use connectors::{Connector, LogConnector, MqttConnector};

#[derive(Serialize)]
struct Report<'a> {
    schema: u32,
    ts: i64,
    gateway_id: &'a str,
    hostname: &'a str,
    version: &'static str,
    uptime_s: u64,
    load: [f64; 3],
    cpu_pct: Option<f64>,
    mem: Option<metrics::Mem>,
    disks: Vec<metrics::Disk>,
    ips: Vec<metrics::Ip>,
    iss_devices: usize,
    mqtt_connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_cert: Option<cert::CertInfo>,
}

fn main() -> ExitCode {
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("gw-monitor {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let cfg = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            log!("ERROR", "config: {e}");
            return ExitCode::from(2);
        }
    };

    let hostname = metrics::hostname();
    let cert_cn = cert::inspect_file(&cfg.api_client_cert).and_then(|c| c.cn);
    let gateway_id = if !cfg.gateway_id.is_empty() {
        cfg.gateway_id.clone()
    } else {
        cert_cn.unwrap_or_else(|| hostname.clone())
    };

    log!("INFO", "gw-monitor {} starting, gateway_id={gateway_id}", env!("CARGO_PKG_VERSION"));
    log!("INFO", "config: {}", cfg.summary());

    // The ISS subscriber always runs: it is the input for iss_devices.
    let iss = iss::Iss::start(&cfg, &hostname);

    let mut outs: Vec<Box<dyn Connector>> = Vec::new();
    if cfg.has("log") {
        outs.push(Box::new(LogConnector));
    }
    if cfg.has("mqtt") {
        if cfg.publish_topic.is_empty() {
            log!("WARN", "connector=mqtt disabled: PUBLISH_TOPIC is empty");
        } else {
            outs.push(Box::new(MqttConnector { client: iss.client.clone(), topic: cfg.publish_topic.clone() }));
        }
    }
    if cfg.has("api") {
        if cfg.api_url.is_empty() {
            log!("WARN", "connector=api disabled: API_URL is empty");
        } else {
            outs.push(Box::new(ApiConnector::start(&cfg)));
        }
    }
    log!("INFO", "active connectors: [{}]", outs.iter().map(|c| c.name()).collect::<Vec<_>>().join(","));

    let mut collector = metrics::Collector::new();
    let interval = Duration::from_secs(cfg.interval_secs);
    // Give MQTT a moment so the first report already includes retained/early device messages.
    thread::sleep(Duration::from_secs(2));

    loop {
        let started = Instant::now();
        let report = Report {
            schema: 1,
            ts: util::now_unix(),
            gateway_id: &gateway_id,
            hostname: &hostname,
            version: env!("CARGO_PKG_VERSION"),
            uptime_s: metrics::uptime_s(),
            load: metrics::load_avg(),
            cpu_pct: collector.cpu_pct(),
            mem: metrics::mem(),
            disks: metrics::disks(&cfg.disk_mounts),
            ips: metrics::ips(&cfg.ifaces),
            iss_devices: iss.device_count(),
            mqtt_connected: iss.connected(),
            client_cert: cert::inspect_file(&cfg.api_client_cert),
        };
        match serde_json::to_string(&report) {
            Ok(json) => {
                for c in outs.iter_mut() {
                    if let Err(e) = c.send(&json) {
                        log!("WARN", "connector={} send failed: {e}", c.name());
                    }
                }
            }
            Err(e) => log!("ERROR", "serialize report: {e}"),
        }
        thread::sleep(interval.saturating_sub(started.elapsed()));
    }
}
