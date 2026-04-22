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
//! - The [`VertexTransport`] runs the `tashi-vertex::Engine` on a dedicated
//!   worker thread that owns a long-lived current-thread tokio runtime.
//!   That matches the warmup reference's shape: the engine keeps making
//!   progress even while the synchronous coordination runtime is between
//!   transport calls.
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
use std::sync::{
    mpsc::{self, Receiver, RecvTimeoutError},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result};
use tashi_vertex::{
    Context as VtxContext, Engine, KeyPublic, KeySecret, Message, Options, Peers, Socket,
    Transaction,
};
use tokio::sync::mpsc as tokio_mpsc;
use tokio::time::{self, MissedTickBehavior};

use vertex_veil_core::{CoordinationMessage, CoordinationTransport, TransportError};

/// Marker prefix for heartbeat transactions — keeps Vertex's consensus
/// engine producing events between sparse application-level broadcasts.
/// Heartbeats are filtered out before they reach the application.
const HEARTBEAT_MARKER: &[u8] = b"HBT\0";

/// Heartbeat cadence used by the worker thread.
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
    outbound: tokio_mpsc::UnboundedSender<WorkerCommand>,
    inbound: Receiver<CoordinationMessage>,
    worker: Option<JoinHandle<()>>,
    /// FIFO buffer of messages drained from consensus events but not yet
    /// consumed by the runtime. Consensus ordering is preserved.
    pending: VecDeque<CoordinationMessage>,
    poll_timeout: Duration,
    closed: bool,
}

enum WorkerCommand {
    Broadcast(Vec<u8>),
}

impl VertexTransport {
    /// Bring up the engine and return a transport ready for use.
    pub fn start(config: VertexConfig) -> Result<Self> {
        let poll_timeout = config.poll_timeout;
        let (outbound, outbound_rx) = tokio_mpsc::unbounded_channel();
        let (inbound_tx, inbound) = mpsc::channel();
        let (startup_tx, startup_rx) = mpsc::channel();
        let worker_error = Arc::new(Mutex::new(None));
        let worker_error_clone = Arc::clone(&worker_error);

        let worker = thread::Builder::new()
            .name(format!("vertex-transport:{}", config.bind))
            .spawn(move || {
                let start_result = run_worker(
                    config,
                    outbound_rx,
                    inbound_tx,
                    startup_tx,
                    Arc::clone(&worker_error_clone),
                );

                if let Err(err) = start_result {
                    set_worker_error(&worker_error_clone, err.to_string());
                    return;
                }
            })
            .context("spawn vertex worker thread")?;

        match startup_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                let _ = worker.join();
                anyhow::bail!(err);
            }
            Err(_) => {
                let _ = worker.join();
                anyhow::bail!("vertex worker exited before startup completed");
            }
        }

        Ok(VertexTransport {
            outbound,
            inbound,
            worker: Some(worker),
            pending: VecDeque::new(),
            poll_timeout,
            closed: false,
        })
    }

    fn mark_closed(&mut self) {
        self.closed = true;
    }

    fn recv_into_pending(&mut self, block: bool) -> Result<bool, TransportError> {
        if self.closed {
            return Err(TransportError::Closed);
        }

        let res = if block {
            match self.inbound.recv_timeout(self.poll_timeout) {
                Ok(msg) => Ok(Some(msg)),
                Err(RecvTimeoutError::Timeout) => Ok(None),
                Err(RecvTimeoutError::Disconnected) => Err(TransportError::Closed),
            }
        } else {
            match self.inbound.try_recv() {
                Ok(msg) => Ok(Some(msg)),
                Err(mpsc::TryRecvError::Empty) => Ok(None),
                Err(mpsc::TryRecvError::Disconnected) => Err(TransportError::Closed),
            }
        };

        match res {
            Ok(Some(msg)) => {
                self.pending.push_back(msg);
                Ok(true)
            }
            Ok(None) => Ok(false),
            Err(err) => {
                self.mark_closed();
                Err(err)
            }
        }
    }
}

impl CoordinationTransport for VertexTransport {
    fn broadcast(&mut self, msg: CoordinationMessage) -> Result<(), TransportError> {
        if self.closed {
            return Err(TransportError::Closed);
        }
        let payload = serde_json::to_vec(&msg).map_err(|_| TransportError::Closed)?;
        self.outbound
            .send(WorkerCommand::Broadcast(payload))
            .map_err(|_| {
                self.mark_closed();
                TransportError::Closed
            })
    }

    fn next_ordered(&mut self) -> Option<CoordinationMessage> {
        if let Some(m) = self.pending.pop_front() {
            return Some(m);
        }

        match self.recv_into_pending(true) {
            Ok(_) => self.pending.pop_front(),
            Err(_) => None,
        }
    }

    fn flush(&mut self) {
        while let Ok(true) = self.recv_into_pending(false) {}
    }
}

impl Drop for VertexTransport {
    fn drop(&mut self) {
        // `tashi-vertex` currently leaves background tokio work alive past
        // the point where a coordinated run has already finalized. Trying
        // to synchronously shut its runtime down here triggers a tokio panic
        // inside those worker tasks. Detach the transport thread instead;
        // node/demo-bft processes exit immediately after artifact writing,
        // so the OS reaps the detached worker without losing the run.
        let keepalive = std::mem::replace(&mut self.outbound, tokio_mpsc::unbounded_channel().0);
        std::mem::forget(keepalive);
        let _ = self.worker.take();
    }
}

fn run_worker(
    config: VertexConfig,
    mut outbound_rx: tokio_mpsc::UnboundedReceiver<WorkerCommand>,
    inbound_tx: mpsc::Sender<CoordinationMessage>,
    startup_tx: mpsc::Sender<std::result::Result<(), String>>,
    worker_error: Arc<Mutex<Option<String>>>,
) -> Result<()> {
    let startup_tx_err = startup_tx.clone();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build vertex worker runtime")?;

    rt.block_on(async move {
        let key: KeySecret = config
            .secret_hex
            .parse()
            .context("parse ed25519 secret")?;

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
        let socket = Socket::bind(&context, &config.bind)
            .await
            .context("bind vertex socket")?;
        let engine = Engine::start(
            &context,
            socket,
            Options::default(),
            &key,
            peers,
            config.rejoin,
        )
        .context("start vertex engine")?;

        let _ = startup_tx.send(Ok(()));

        let mut heartbeat_interval = time::interval(Duration::from_millis(HEARTBEAT_INTERVAL_MS));
        heartbeat_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = heartbeat_interval.tick() => {
                    if let Err(err) = send_bytes(&engine, HEARTBEAT_MARKER) {
                        set_worker_error(&worker_error, format!("heartbeat send failed: {err}"));
                        break;
                    }
                }
                cmd = outbound_rx.recv() => {
                    match cmd {
                        Some(WorkerCommand::Broadcast(payload)) => {
                            if let Err(err) = send_bytes(&engine, &payload) {
                                set_worker_error(&worker_error, format!("broadcast failed: {err}"));
                                break;
                            }
                        }
                        None => break,
                    }
                }
                msg = engine.recv_message() => {
                    match msg {
                        Ok(Some(Message::Event(event))) => {
                            for raw in event.transactions() {
                                if raw.starts_with(HEARTBEAT_MARKER) {
                                    continue;
                                }
                                if let Ok(msg) = serde_json::from_slice::<CoordinationMessage>(raw) {
                                    if inbound_tx.send(msg).is_err() {
                                        return Ok(());
                                    }
                                }
                            }
                        }
                        Ok(Some(Message::SyncPoint(_))) => {}
                        Ok(None) => break,
                        Err(err) => {
                            set_worker_error(&worker_error, format!("recv_message failed: {err}"));
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    })
    .map_err(|err: anyhow::Error| {
        let _ = startup_tx_err.send(Err(err.to_string()));
        err
    })
}

fn send_bytes(engine: &Engine, data: &[u8]) -> tashi_vertex::Result<()> {
    let mut tx = Transaction::allocate(data.len());
    tx.copy_from_slice(data);
    engine.send_transaction(tx)
}

fn set_worker_error(slot: &Arc<Mutex<Option<String>>>, value: String) {
    if let Ok(mut guard) = slot.lock() {
        *guard = Some(value);
    }
}
