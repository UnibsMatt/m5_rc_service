//! `api` connector: HTTP(S) POST/PUT of each report, with optional mutual TLS.
//!
//! Runs on its own thread so a slow or unreachable API never delays collection.
//! Failed reports are kept in a bounded in-memory queue and flushed in order once the
//! API answers again. The client cert/key/CA are re-read whenever their mtime changes.

use std::collections::VecDeque;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use rustls::RootCertStore;
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

use super::Connector;
use crate::cert;
use crate::config::Config;
use crate::log;

/// Handle used by the main loop; it only enqueues.
pub struct ApiConnector {
    tx: Sender<String>,
}

impl ApiConnector {
    pub fn start(cfg: &Config) -> Self {
        let (tx, rx) = mpsc::channel();
        let worker = Worker::new(cfg);
        thread::Builder::new()
            .name("api".into())
            .spawn(move || worker.run(rx))
            .expect("spawn api thread");
        ApiConnector { tx }
    }
}

impl Connector for ApiConnector {
    fn name(&self) -> &'static str {
        "api"
    }
    fn send(&mut self, json: &str) -> Result<(), String> {
        self.tx.send(json.to_string()).map_err(|_| "api worker stopped".into())
    }
}

enum Outcome {
    Sent,
    Rejected(String),  // 4xx: the request itself is wrong -> drop, don't retry
    Transient(String), // network / TLS / 5xx -> keep and retry later
}

type Stamp = Option<SystemTime>;

struct Worker {
    url: String,
    method: String,
    token: String,
    headers: Vec<(String, String)>,
    timeout: Duration,
    retries: u32,
    retry_every: Duration,
    queue_max: usize,
    cert_path: String,
    key_path: String,
    ca_path: String,
    system_ca: String,
    use_tls: bool,

    agent: Option<ureq::Agent>,
    loaded: (Stamp, Stamp, Stamp),
    waiting_logged: bool,
    queue: VecDeque<String>,
    dropped: u64,
    next_attempt: Instant,
    failing: bool,
}

fn mtime(path: &str) -> Stamp {
    if path.is_empty() {
        return None;
    }
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl Worker {
    fn new(cfg: &Config) -> Self {
        Worker {
            url: cfg.api_url.clone(),
            method: cfg.api_method.clone(),
            token: cfg.api_token.clone(),
            headers: cfg.api_headers.clone(),
            timeout: Duration::from_secs(cfg.api_timeout_secs.max(1)),
            retries: cfg.api_retries,
            retry_every: Duration::from_secs(cfg.interval_secs.clamp(5, 60)),
            queue_max: cfg.api_queue_max,
            cert_path: cfg.api_client_cert.clone(),
            key_path: cfg.api_client_key.clone(),
            ca_path: cfg.api_ca_file.clone(),
            system_ca: cfg.api_system_ca.clone(),
            use_tls: cfg.api_url.to_ascii_lowercase().starts_with("https://"),
            agent: None,
            loaded: (None, None, None),
            waiting_logged: false,
            queue: VecDeque::new(),
            dropped: 0,
            next_attempt: Instant::now(),
            failing: false,
        }
    }

    fn run(mut self, rx: Receiver<String>) {
        loop {
            match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(item) => self.push(item),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            while let Ok(item) = rx.try_recv() {
                self.push(item);
            }
            if !self.queue.is_empty() && Instant::now() >= self.next_attempt {
                self.flush();
            }
        }
    }

    fn push(&mut self, item: String) {
        if self.queue.len() >= self.queue_max {
            self.queue.pop_front();
            self.dropped += 1;
            if self.dropped == 1 || self.dropped % 100 == 0 {
                log!("WARN", "connector=api queue full ({}), dropped {} oldest report(s) so far", self.queue_max, self.dropped);
            }
        }
        self.queue.push_back(item);
    }

    fn flush(&mut self) {
        if !self.ensure_agent() {
            self.next_attempt = Instant::now() + self.retry_every;
            return;
        }
        let (mut sent, mut rejected) = (0usize, 0usize);
        while let Some(body) = self.queue.front().cloned() {
            match self.send_with_retries(&body) {
                Outcome::Sent => {
                    self.queue.pop_front();
                    sent += 1;
                }
                Outcome::Rejected(e) => {
                    self.queue.pop_front();
                    rejected += 1;
                    log!("ERROR", "connector=api rejected, report dropped: {e}");
                }
                Outcome::Transient(e) => {
                    log!("WARN", "connector=api failed after {} retries: {e} (queued: {})", self.retries, self.queue.len());
                    self.failing = true;
                    self.next_attempt = Instant::now() + self.retry_every;
                    return;
                }
            }
        }
        if self.failing {
            log!("INFO", "connector=api reachable again: {sent} report(s) delivered, {rejected} rejected");
            self.failing = false;
        }
    }

    fn send_with_retries(&self, body: &str) -> Outcome {
        let mut last = Outcome::Transient("not attempted".into());
        for attempt in 0..=self.retries {
            if attempt > 0 {
                thread::sleep(Duration::from_secs(1 << (attempt - 1).min(5)));
            }
            last = self.send_once(body);
            if !matches!(last, Outcome::Transient(_)) {
                return last;
            }
        }
        last
    }

    fn send_once(&self, body: &str) -> Outcome {
        let agent = self.agent.as_ref().expect("agent ready");
        let mut req = agent
            .request(&self.method, &self.url)
            .set("Content-Type", "application/json");
        if !self.token.is_empty() {
            req = req.set("Authorization", &format!("Bearer {}", self.token));
        }
        for (k, v) in &self.headers {
            req = req.set(k, v);
        }
        match req.send_string(body) {
            Ok(_) => Outcome::Sent,
            Err(ureq::Error::Status(code, resp)) => {
                let msg = format!("HTTP {code} {}", resp.status_text());
                // 408/429 are "try again later"; other 4xx mean the request is wrong.
                if (400..500).contains(&code) && code != 408 && code != 429 {
                    Outcome::Rejected(msg)
                } else {
                    Outcome::Transient(msg)
                }
            }
            Err(ureq::Error::Transport(t)) => Outcome::Transient(t.to_string()),
        }
    }

    /// Build (or rebuild after cert rotation) the HTTP agent. Returns false while waiting
    /// for certificate files to appear.
    fn ensure_agent(&mut self) -> bool {
        if !self.use_tls {
            if self.agent.is_none() {
                self.agent = Some(self.builder().build());
            }
            return true;
        }
        let stamps = (mtime(&self.cert_path), mtime(&self.key_path), mtime(&self.ca_path));
        if self.agent.is_some() && stamps == self.loaded {
            return true;
        }
        let have_client_cert = !self.cert_path.is_empty() && !self.key_path.is_empty();
        if have_client_cert && (stamps.0.is_none() || stamps.1.is_none()) {
            if !self.waiting_logged {
                log!("WARN", "connector=api waiting for {} / {} (queued reports are kept)", self.cert_path, self.key_path);
                self.waiting_logged = true;
            }
            return false;
        }
        match self.build_tls() {
            Ok(cfg) => {
                let reload = self.agent.is_some();
                self.agent = Some(self.builder().tls_config(Arc::new(cfg)).build());
                self.loaded = stamps;
                self.waiting_logged = false;
                log!("INFO", "connector=api TLS {} (client cert: {})",
                    if reload { "reloaded" } else { "ready" },
                    if have_client_cert { self.cert_path.as_str() } else { "none" });
                true
            }
            Err(e) => {
                log!("ERROR", "connector=api TLS setup failed: {e}");
                self.agent = None;
                self.loaded = stamps; // don't spam: retry only when files change
                false
            }
        }
    }

    fn builder(&self) -> ureq::AgentBuilder {
        ureq::AgentBuilder::new()
            .timeout_connect(self.timeout)
            .timeout(self.timeout)
            .user_agent(concat!("gw-monitor/", env!("CARGO_PKG_VERSION")))
    }

    fn build_tls(&self) -> Result<rustls::ClientConfig, String> {
        let mut roots = RootCertStore::empty();
        if let Ok(iter) = CertificateDer::pem_file_iter(&self.system_ca) {
            for c in iter.flatten() {
                let _ = roots.add(c);
            }
        }
        if !self.ca_path.is_empty() {
            let mut n = 0;
            for c in CertificateDer::pem_file_iter(&self.ca_path)
                .map_err(|e| format!("API_CA_FILE {}: {e}", self.ca_path))?
            {
                let c = c.map_err(|e| format!("API_CA_FILE {}: {e}", self.ca_path))?;
                roots.add(c).map_err(|e| format!("API_CA_FILE {}: {e}", self.ca_path))?;
                n += 1;
            }
            if n == 0 {
                return Err(format!("API_CA_FILE {}: no certificates found", self.ca_path));
            }
        }
        if roots.is_empty() {
            return Err("no trusted CA certificates (install ca-certificates-bundle or set API_CA_FILE)".into());
        }

        let builder = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_root_certificates(roots);

        if self.cert_path.is_empty() || self.key_path.is_empty() {
            return Ok(builder.with_no_client_auth());
        }

        let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(&self.cert_path)
            .map_err(|e| format!("{}: {e}", self.cert_path))?
            .collect::<Result<_, _>>()
            .map_err(|e| format!("{}: {e}", self.cert_path))?;
        if chain.is_empty() {
            return Err(format!("{}: no certificate found", self.cert_path));
        }
        let key = PrivateKeyDer::from_pem_file(&self.key_path)
            .map_err(|e| format!("{}: {e} (key must be unencrypted PEM)", self.key_path))?;

        if let Ok(meta) = fs::metadata(&self.key_path) {
            let mode = meta.permissions().mode() & 0o777;
            if mode & 0o007 != 0 {
                log!("WARN", "connector=api key file {} is mode {:04o} (accessible by all users - insecure)", self.key_path, mode);
            }
        }
        if let Some(info) = cert::inspect(&chain[0]) {
            match info.days_left {
                Some(d) if d < 0 => log!("ERROR", "connector=api client cert {:?} EXPIRED", info.cn),
                Some(d) if d < 14 => log!("WARN", "connector=api client cert {:?} expires in {d} day(s)", info.cn),
                _ => {}
            }
        }

        builder
            .with_client_auth_cert(chain, key)
            .map_err(|e| format!("client cert/key: {e}"))
    }
}
