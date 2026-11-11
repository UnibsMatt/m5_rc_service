//! Output connectors. Each one receives the same serialized report every interval.
//! A failure in one connector never affects the others.

pub mod api;

use rumqttc::{Client, QoS};

pub trait Connector {
    fn name(&self) -> &'static str;
    fn send(&mut self, json: &str) -> Result<(), String>;
}

/// `log`: one JSON line on stdout -> /var/log/gw-monitor.log via OpenRC output_log.
pub struct LogConnector;

impl Connector for LogConnector {
    fn name(&self) -> &'static str {
        "log"
    }
    fn send(&mut self, json: &str) -> Result<(), String> {
        println!("{json}");
        Ok(())
    }
}

/// `mqtt`: publish (retained, QoS 1) on PUBLISH_TOPIC, reusing the ISS client connection.
pub struct MqttConnector {
    pub client: Client,
    pub topic: String,
}

impl Connector for MqttConnector {
    fn name(&self) -> &'static str {
        "mqtt"
    }
    fn send(&mut self, json: &str) -> Result<(), String> {
        self.client
            .try_publish(self.topic.clone(), QoS::AtLeastOnce, true, json.as_bytes().to_vec())
            .map_err(|e| e.to_string())
    }
}
