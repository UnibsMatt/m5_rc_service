//! MQTT subscriber that counts distinct device MACs seen on the ISS topics,
//! e.g. `/ISS/8A:8D:04:43:3A:10/debug`. Only the topic is used; payloads are ignored.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rumqttc::{Client, Event, MqttOptions, Packet, QoS};

use crate::config::Config;
use crate::log;

/// A subscription filter split into levels, plus the index of the `+` that holds the MAC.
struct Filter {
    levels: Vec<String>,
    mac_level: usize,
}

impl Filter {
    fn new(filter: &str) -> Self {
        let levels: Vec<String> = filter.split('/').map(str::to_string).collect();
        let mac_level = levels.iter().position(|l| l == "+").unwrap_or(0);
        Filter { levels, mac_level }
    }

    /// If `topic` matches this filter, return the raw topic level at the MAC position.
    fn extract<'a>(&self, topic: &'a str) -> Option<&'a str> {
        let parts: Vec<&str> = topic.split('/').collect();
        for (i, f) in self.levels.iter().enumerate() {
            match f.as_str() {
                "#" => break,
                "+" => {
                    parts.get(i)?;
                }
                lit => {
                    if parts.get(i) != Some(&lit) {
                        return None;
                    }
                }
            }
            if i + 1 == self.levels.len() && parts.len() != self.levels.len() {
                return None;
            }
        }
        parts.get(self.mac_level).copied()
    }
}

/// `8A:8D:04:43:3A:10`, `8a-8d-...`, `8a8d04433a10` -> `8A8D04433A10`; anything else -> None.
pub fn normalize_mac(raw: &str) -> Option<String> {
    let s: String = raw
        .chars()
        .filter(|c| !matches!(c, ':' | '-' | '.'))
        .collect::<String>()
        .to_ascii_uppercase();
    (s.len() == 12 && s.chars().all(|c| c.is_ascii_hexdigit())).then_some(s)
}

pub struct Iss {
    devices: Arc<Mutex<HashMap<String, Instant>>>,
    connected: Arc<AtomicBool>,
    ttl: Option<Duration>,
    pub client: Client,
}

impl Iss {
    pub fn start(cfg: &Config, hostname: &str) -> Self {
        let client_id = if cfg.mqtt_client_id.is_empty() {
            format!("gw-monitor-{hostname}")
        } else {
            cfg.mqtt_client_id.clone()
        };
        let mut opts = MqttOptions::new(client_id, cfg.mqtt_host.clone(), cfg.mqtt_port);
        opts.set_credentials(cfg.mqtt_user.clone(), cfg.mqtt_pass.clone());
        opts.set_keep_alive(Duration::from_secs(30));
        opts.set_clean_session(true);
        // Debug payloads can be large; the rumqttc default (10 KiB) would drop the connection.
        opts.set_max_packet_size(cfg.mqtt_max_packet, cfg.mqtt_max_packet);

        let (client, mut connection) = Client::new(opts, 256);
        let devices = Arc::new(Mutex::new(HashMap::new()));
        let connected = Arc::new(AtomicBool::new(false));

        let filters: Vec<Filter> = cfg.iss_topics.iter().map(|t| Filter::new(t)).collect();
        let topics = cfg.iss_topics.clone();
        let (dev, conn, sub) = (devices.clone(), connected.clone(), client.clone());
        let broker = format!("{}:{}", cfg.mqtt_host, cfg.mqtt_port);

        thread::Builder::new()
            .name("mqtt".into())
            .spawn(move || {
                let mut last_err = String::new();
                for event in connection.iter() {
                    match event {
                        Ok(Event::Incoming(Packet::ConnAck(_))) => {
                            conn.store(true, Ordering::Relaxed);
                            last_err.clear();
                            log!("INFO", "mqtt connected to {broker}, subscribing {}", topics.join(","));
                            // clean_session=true -> must re-subscribe after every (re)connect.
                            for t in &topics {
                                if let Err(e) = sub.try_subscribe(t.clone(), QoS::AtMostOnce) {
                                    log!("ERROR", "mqtt subscribe {t}: {e}");
                                }
                            }
                        }
                        Ok(Event::Incoming(Packet::SubAck(ack))) => {
                            if ack.return_codes.iter().any(|c| matches!(c, rumqttc::SubscribeReasonCode::Failure)) {
                                log!("ERROR", "mqtt subscription rejected by broker (check ACL for user)");
                            }
                        }
                        Ok(Event::Incoming(Packet::Publish(p))) => {
                            if let Some(mac) = filters
                                .iter()
                                .find_map(|f| f.extract(&p.topic))
                                .and_then(normalize_mac)
                            {
                                let mut map = dev.lock().unwrap();
                                if map.insert(mac.clone(), Instant::now()).is_none() {
                                    log!("INFO", "new ISS device {mac} (total {})", map.len());
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(e) => {
                            conn.store(false, Ordering::Relaxed);
                            let msg = e.to_string();
                            if msg != last_err {
                                log!("WARN", "mqtt {broker}: {msg} (retrying every 5s)");
                                last_err = msg;
                            }
                            thread::sleep(Duration::from_secs(5));
                        }
                    }
                }
            })
            .expect("spawn mqtt thread");

        Iss {
            devices,
            connected,
            ttl: (cfg.device_ttl_secs > 0).then(|| Duration::from_secs(cfg.device_ttl_secs)),
            client,
        }
    }

    /// Distinct devices (all ever seen, or seen within DEVICE_TTL_SECS).
    pub fn device_count(&self) -> usize {
        let mut map = self.devices.lock().unwrap();
        if let Some(ttl) = self.ttl {
            map.retain(|_, seen| seen.elapsed() <= ttl);
        }
        map.len()
    }

    pub fn connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_mac_from_iss_topics() {
        let debug = Filter::new("/ISS/+/debug");
        let data = Filter::new("/ISS/+/data");
        let any = Filter::new("/ISS/+/#");
        let t = "/ISS/8A:8D:04:43:3A:10/debug";
        assert_eq!(debug.extract(t), Some("8A:8D:04:43:3A:10"));
        assert_eq!(data.extract(t), None);
        assert_eq!(any.extract(t), Some("8A:8D:04:43:3A:10"));
        assert_eq!(debug.extract("/ISS/8A:8D:04:43:3A:10/debug/extra"), None);
        assert_eq!(debug.extract("/iss/8A:8D:04:43:3A:10/debug"), None);
        assert_eq!(debug.extract("ISS/8A:8D:04:43:3A:10/debug"), None);
    }

    #[test]
    fn mac_normalization() {
        assert_eq!(normalize_mac("8A:8D:04:43:3A:10").as_deref(), Some("8A8D04433A10"));
        assert_eq!(normalize_mac("8a-8d-04-43-3a-10").as_deref(), Some("8A8D04433A10"));
        assert_eq!(normalize_mac("8a8d04433a10").as_deref(), Some("8A8D04433A10"));
        assert_eq!(normalize_mac("gateway"), None);
        assert_eq!(normalize_mac("8A:8D:04:43:3A"), None);
    }
}
