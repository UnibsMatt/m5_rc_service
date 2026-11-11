//! Configuration from environment variables (set in /etc/conf.d/gw-monitor).

use std::env;
use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct Config {
    pub connectors: Vec<String>,
    pub gateway_id: String, // empty = auto (cert CN, then hostname)

    pub mqtt_host: String,
    pub mqtt_port: u16,
    pub mqtt_user: String,
    pub mqtt_pass: String,
    pub mqtt_client_id: String, // empty = gw-monitor-<hostname>
    pub mqtt_max_packet: usize,
    pub iss_topics: Vec<String>,
    pub publish_topic: String,

    pub api_url: String,
    pub api_method: String,
    pub api_token: String,
    pub api_headers: Vec<(String, String)>,
    pub api_timeout_secs: u64,
    pub api_retries: u32,
    pub api_queue_max: usize,
    pub api_client_cert: String,
    pub api_client_key: String,
    pub api_ca_file: String,
    pub api_system_ca: String,

    pub interval_secs: u64,
    pub device_ttl_secs: u64,
    pub ifaces: Vec<String>, // empty = all
    pub disk_mounts: Vec<String>,
}

pub const KNOWN_CONNECTORS: &[&str] = &["log", "mqtt", "api"];

fn s(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

fn n<T: FromStr>(key: &str, default: T) -> Result<T, String> {
    match env::var(key) {
        Ok(v) if !v.trim().is_empty() => v
            .trim()
            .parse()
            .map_err(|_| format!("{key}: invalid number {v:?}")),
        _ => Ok(default),
    }
}

fn list(key: &str, default: &str) -> Vec<String> {
    s(key, default)
        .split(',')
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let connectors: Vec<String> = list("CONNECTORS", "log,mqtt")
            .into_iter()
            .map(|c| c.to_ascii_lowercase())
            .collect();
        for c in &connectors {
            if !KNOWN_CONNECTORS.contains(&c.as_str()) {
                return Err(format!(
                    "CONNECTORS: unknown connector {c:?} (known: {})",
                    KNOWN_CONNECTORS.join(",")
                ));
            }
        }

        let iss_topics = list("ISS_TOPICS", "/ISS/+/data,/ISS/+/debug");
        for t in &iss_topics {
            if !t.split('/').any(|l| l == "+") {
                return Err(format!(
                    "ISS_TOPICS: {t:?} has no '+' level; the '+' marks where the MAC address is"
                ));
            }
        }

        let api_headers = s("API_HEADERS", "")
            .split(';')
            .filter_map(|kv| {
                let (k, v) = kv.split_once('=')?;
                let k = k.trim();
                (!k.is_empty()).then(|| (k.to_string(), v.trim().to_string()))
            })
            .collect();

        let interval_secs = n("INTERVAL_SECS", 30u64)?;
        if interval_secs == 0 {
            return Err("INTERVAL_SECS must be > 0".into());
        }

        Ok(Config {
            connectors,
            gateway_id: s("GATEWAY_ID", "").trim().to_string(),
            mqtt_host: s("MQTT_HOST", "127.0.0.1"),
            mqtt_port: n("MQTT_PORT", 31883u16)?,
            mqtt_user: s("MQTT_USER", "iss"),
            mqtt_pass: s("MQTT_PASS", "iss123."),
            mqtt_client_id: s("MQTT_CLIENT_ID", ""),
            mqtt_max_packet: n("MQTT_MAX_PACKET", 1024 * 1024usize)?,
            iss_topics,
            publish_topic: s("PUBLISH_TOPIC", "gateway/status").trim().to_string(),
            api_url: s("API_URL", "").trim().to_string(),
            api_method: s("API_METHOD", "POST").trim().to_ascii_uppercase(),
            api_token: s("API_TOKEN", ""),
            api_headers,
            api_timeout_secs: n("API_TIMEOUT_SECS", 10u64)?,
            api_retries: n("API_RETRIES", 3u32)?,
            api_queue_max: n("API_QUEUE_MAX", 100usize)?.max(1),
            api_client_cert: s("API_CLIENT_CERT", "/dev/shm/client.crt"),
            api_client_key: s("API_CLIENT_KEY", "/dev/shm/client.key"),
            api_ca_file: s("API_CA_FILE", ""),
            api_system_ca: s("API_SYSTEM_CA", "/etc/ssl/certs/ca-certificates.crt"),
            interval_secs,
            device_ttl_secs: n("DEVICE_TTL_SECS", 0u64)?,
            ifaces: list("IFACE", ""),
            disk_mounts: list("DISK_MOUNTS", "/"),
        })
    }

    pub fn has(&self, connector: &str) -> bool {
        self.connectors.iter().any(|c| c == connector)
    }

    /// One-line summary for the startup log, secrets redacted.
    pub fn summary(&self) -> String {
        let red = |v: &str| if v.is_empty() { "<unset>" } else { "<redacted>" };
        format!(
            "connectors={} mqtt={}@{}:{} pass={} iss_topics={} publish_topic={:?} \
             api_url={:?} api_token={} api_cert={} api_key={} api_ca={:?} \
             interval={}s device_ttl={}s iface={} disks={}",
            self.connectors.join(","),
            self.mqtt_user,
            self.mqtt_host,
            self.mqtt_port,
            red(&self.mqtt_pass),
            self.iss_topics.join(","),
            self.publish_topic,
            self.api_url,
            red(&self.api_token),
            self.api_client_cert,
            self.api_client_key,
            self.api_ca_file,
            self.interval_secs,
            self.device_ttl_secs,
            if self.ifaces.is_empty() { "<all>".to_string() } else { self.ifaces.join(",") },
            self.disk_mounts.join(","),
        )
    }
}
