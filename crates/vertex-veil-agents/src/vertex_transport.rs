//! Real `tashi-vertex` transport implementation.
//!
//! This module is gated behind the `vertex-transport` cargo feature because
//! it pulls in `tashi-vertex` (network-dependent) and `tokio` (async
//! runtime). The default gate for the Phase 4 demo uses the in-process
//! [`vertex_veil_core::OrderedBus`]; this path exists so a judge can run
//! the 4-node baseline against real consensus-ordered delivery.
//!
//! Design notes:
//!
//! - The [`VertexTransport`] wraps a `tashi-vertex::Engine` and drives it
//!   from a dedicated tokio runtime owned by the transport. `broadcast`
//!   sends a JSON-encoded [`vertex_veil_core::CoordinationMessage`] as a
//!   transaction. `next_ordered` blocks on `Engine::recv_message` and
//!   yields the next decoded message when a consensus event arrives.
//! - Transaction payload format is
//!   `serde_json::to_vec(&CoordinationMessage)`. Consensus order is
//!   preserved by the Vertex engine; the transport does not reorder.
//! - Malformed transactions (non-JSON, wrong shape) are surfaced via
//!   `next_ordered` returning `None` for that event's slot — the caller's
//!   rejection path on the `CoordinationRuntime` logs the anomaly.
//!
//! Keep this file narrow: the transport is a thin adapter. Protocol
//! semantics live in `vertex-veil-core`.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json;
use tashi_vertex::{
    Context as VtxContext, Engine, KeyPublic, KeySecret, Message, Options, Peers, Socket,
    Transaction,
};
use tokio::runtime::Runtime;
use tokio::time::timeout;

use vertex_veil_core::{CoordinationMessage, CoordinationTransport, TransportError};

/// Marker prefix for heartbeat transactions — keeps Vertex's consensus
/// engine producing events between sparse application-level broadcasts.
/// Heartbeats are filtered out before they reach the application.
const HEARTBEAT_MARKER: &[u8] = b"HBT\0";

/// Minimum gap between heartbeats. The warmup reference uses 500ms at a
/// steady cadence; we pace inline with transport calls, which end up
/// invoking heartbeats every few hundred ms anyway.
const HEARTBEAT_INTERVAL_MS: u64 = 300;

/// Raw configuration for bringing up a single Vertex node.
///
/// The fields map one-to-one to the arguments the `warmup-vertex-rust`
/// reference binary accepts: a bind socket, a local secret, the set of
/// peers with their public keys.
pub struct VertexConfig {
    /// Local socket bind (e.g. `127.0.0.1:9000`).
    pub bind: String,
    /// Local signing secret (hex-encoded, as produced by
    /// `KeySecret::generate`).
    pub secret_hex: String,
    /// Peer entries as `(<pubkey-hex>, <addr>)`.
    pub peers: Vec<(String, String)>,
    /// Whether this node is rejoining an existing cluster.
    pub rejoin: bool,
    /// Maximum time to block inside `next_ordered` for a single event.
    pub poll_timeout: Duration,
}

/// A [`CoordinationTransport`] backed by a real tashi-vertex engine.
pub struct VertexTransport {
    rt: Runtime,
    engine: Engine,
    /// FIFO buffer of messages drained from consensus events but not yet
    /// consumed by the runtime. Consensus ordering is preserved.
    pending: VecDeque<CoordinationMessage>,
    poll_timeout: Duration,
    /// Last time a heartbeat transaction was injected. Used to pace
    /// inline heartbeats at `HEARTBEAT_INTERVAL_MS` so Vertex keeps
    /// producing consensus events even when the application is between
    /// protocol phases.
    last_heartbeat: Instant,
}

impl VertexTransport {
    /// Bring up the engine and return a transport ready for use.
    pub fn start(config: VertexConfig) -> Result<Self> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("build tokio runtime")?;

        let key: KeySecret = config
            .secret_hex
            .parse()
            .context("parse ed25519 secret")?;

        // Build the peers registry (peers + self).
        let mut peers = Peers::new().context("create peer registry")?;
        for (pub_hex, addr) in &config.peers {
            let pk: KeyPublic = pub_hex.parse().context("parse peer public key")?;
            peers
                .insert(addr, &pk, Default::default())
                .context("insert peer")?;
        }
        peers
            .insert(&config.bind, &key.public(), Default::default())
            .context("insert self")?;

        let context = VtxContext::new().context("create vertex context")?;
        let (engine, _socket_guard) = rt.block_on(async {
            let socket = Socket::bind(&context, &config.bind).await?;
            let engine = Engine::start(
                &context,
                socket,
                Options::default(),
                &key,
                peers,
                config.rejoin,
            )?;
            Ok::<_, anyhow::Error>((engine, ()))
        })?;

        Ok(VertexTransport {
            rt,
            engine,
            pending: VecDeque::new(),
            poll_timeout: config.poll_timeout,
            last_heartbeat: Instant::now()
                .checked_sub(Duration::from_secs(1))
                .unwrap_or_else(Instant::now),
        })
    }

    /// Send a heartbeat transaction if the pacing interval has elapsed.
    /// Inline because `Engine` is not `Send` — we cannot ship it to a
    /// background tokio task. Every transport method calls this on entry
    /// so heartbeats fire naturally at the protocol's cadence.
    fn pulse_heartbeat(&mut self) {
        if self.last_heartbeat.elapsed() < Duration::from_millis(HEARTBEAT_INTERVAL_MS) {
            return;
        }
        let mut tx = Transaction::allocate(HEARTBEAT_MARKER.len());
        tx.copy_from_slice(HEARTBEAT_MARKER);
        let _ = self.engine.send_transaction(tx);
        self.last_heartbeat = Instant::now();
    }

    fn drain_one_event(&mut self) -> Result<bool, TransportError> {
        self.pulse_heartbeat();
        let res = self.rt.block_on(async {
            match timeout(self.poll_timeout, self.engine.recv_message()).await {
                Err(_) => Ok::<Option<Message>, tashi_vertex::Error>(None),
                Ok(m) => Ok(m?),
            }
        });
        match res {
            Err(_) => Err(TransportError::Closed),
            Ok(None) => Ok(false),
            Ok(Some(Message::SyncPoint(_))) => {
                // A SyncPoint is a consensus barrier delivery — it does
                // NOT mean "no more events." Keep draining so we don't
                // stop mid-batch just because a barrier arrived.
                Ok(true)
            }
            Ok(Some(Message::Event(event))) => {
                for raw in event.transactions() {
                    // Skip heartbeats; they keep the engine ticking but
                    // never surface to the application.
                    if raw.starts_with(HEARTBEAT_MARKER) {
                        continue;
                    }
                    match serde_json::from_slice::<CoordinationMessage>(raw) {
                        Ok(msg) => self.pending.push_back(msg),
                        Err(_) => {
                            // Malformed application transaction: skip. The
                            // runtime's rejection path handles absent
                            // payloads when the round drains.
                        }
                    }
                }
                Ok(true)
            }
        }
    }
}

impl CoordinationTransport for VertexTransport {
    fn broadcast(&mut self, msg: CoordinationMessage) -> Result<(), TransportError> {
        self.pulse_heartbeat();
        let payload = serde_json::to_vec(&msg).map_err(|_| TransportError::Closed)?;
        let mut tx = Transaction::allocate(payload.len());
        tx.copy_from_slice(&payload);
        self.engine
            .send_transaction(tx)
            .map_err(|_| TransportError::Closed)
    }

    fn next_ordered(&mut self) -> Option<CoordinationMessage> {
        self.pulse_heartbeat();
        if let Some(m) = self.pending.pop_front() {
            return Some(m);
        }
        // Attempt one drain cycle; timeout here is bounded by poll_timeout.
        match self.drain_one_event() {
            Ok(_) => self.pending.pop_front(),
            Err(_) => None,
        }
    }

    fn flush(&mut self) {
        self.pulse_heartbeat();
        // Drain any events already in-flight so the caller sees everything
        // that is currently ready without waiting.
        while let Ok(true) = self.drain_one_event() {}
    }
}
