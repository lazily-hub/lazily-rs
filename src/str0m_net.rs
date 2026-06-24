//! Concrete **networked** (non-loopback) str0m DataChannel backend
//! (#lzwebrtcnet / #jpf1), behind the `webrtc-str0m` feature.
//!
//! Where [`Str0mLoopback`](crate::Str0mLoopback) drives two `Rtc` instances in a
//! single thread over a synthetic clock and an in-memory packet route (fully
//! deterministic, no sockets), [`Str0mNet`] drives **one** `Rtc` over a real UDP
//! socket on a background driver thread. The sans-IO pump loop is the same shape
//! as the loopback — drain [`Rtc::poll_output`], route [`Output::Transmit`] to
//! the socket, feed inbound datagrams back via [`Rtc::handle_input`] — but the
//! transport, clock, and timers are real, so this backend can reach a peer on a
//! different host.
//!
//! The SDP offer/answer + trickled ICE handshake is exchanged by the caller,
//! typically over the [`SignalingClient`](crate::SignalingClient) #yxjw relay:
//!
//! ```no_run
//! # #[cfg(feature = "webrtc-str0m")]
//! # fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! use lazily::Str0mNet;
//!
//! // Offerer: bind a socket, produce the SDP offer to send via signaling.
//! let (offerer, offer_sdp) = Str0mNet::offer("0.0.0.0:0".parse()?)?;
//! // ... send `offer_sdp` to the peer; trickle `offerer.local_candidate()` too.
//! // ... receive the peer's answer SDP and its candidate:
//! # let answer_sdp = String::new();
//! # let peer_candidate = String::new();
//! offerer.accept_answer(&answer_sdp)?;
//! offerer.add_remote_candidate(&peer_candidate)?;
//! if offerer.wait_open(std::time::Duration::from_secs(10)) {
//!     // `offerer.channel()` is now a `DataChannel`; wrap it in
//!     // `WebRtcSink` / `WebRtcSource` exactly like the loopback backend.
//! }
//! # Ok(())
//! # }
//! ```
//!
//! Because it needs live two-peer connectivity it cannot be modelled on a
//! synthetic clock; the integration test exercises a real two-socket round trip
//! over `127.0.0.1` (real UDP, real DTLS/SCTP, real timers). A round trip across
//! two hosts through the live signaling Worker is operator-gated.

use std::collections::VecDeque;
use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use str0m::change::{SdpAnswer, SdpOffer, SdpPendingOffer};
use str0m::channel::ChannelId;
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, Input, Output, Rtc};

use crate::webrtc_transport::DataChannel;

/// Error from the networked str0m backend.
#[derive(Debug)]
pub enum Str0mNetError {
    /// UDP socket I/O failed (bind / send / recv).
    Io(std::io::Error),
    /// A str0m API call failed.
    Rtc(str0m::RtcError),
    /// SDP / ICE-candidate parsing or negotiation failed.
    Sdp(String),
    /// The driver thread is gone (peer dropped or shut down).
    Closed,
    /// The driver's outbound frame queue is full — the caller is producing
    /// frames faster than the SCTP channel can drain them. Back off (sleep /
    /// await) and retry `send_frame`. Surfaced before the unbounded queue
    /// growth that would otherwise risk memory exhaustion and silent frame
    /// loss (#lzstr0mframe).
    Backpressure,
    /// `accept_answer` was called on an answerer (only offerers hold a pending
    /// offer to answer).
    NotOfferer,
}

impl std::fmt::Display for Str0mNetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "str0m net io error: {e}"),
            Self::Rtc(e) => write!(f, "str0m net rtc error: {e}"),
            Self::Sdp(e) => write!(f, "str0m net sdp/ice error: {e}"),
            Self::Closed => write!(f, "str0m net driver closed"),
            Self::Backpressure => {
                write!(
                    f,
                    "str0m net outbound queue full (apply flow control and retry)"
                )
            }
            Self::NotOfferer => write!(f, "accept_answer called on a non-offerer peer"),
        }
    }
}

impl std::error::Error for Str0mNetError {}

impl From<std::io::Error> for Str0mNetError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<str0m::RtcError> for Str0mNetError {
    fn from(e: str0m::RtcError) -> Self {
        Self::Rtc(e)
    }
}

/// Commands sent from the public handle to the driver thread (which owns the
/// non-`Sync` `Rtc`).
enum DriverCmd {
    /// Offerer applies the peer's answer SDP.
    AcceptAnswer(String),
    /// Feed a trickled remote ICE candidate (SDP `candidate:` string).
    AddRemoteCandidate(String),
    /// Application frame to write on the data channel once it is open.
    Send(Vec<u8>),
    /// Tear down: disconnect the `Rtc` and exit the driver loop.
    Shutdown,
}

/// Maximum number of unsent frames buffered in the driver at once. Above this
/// threshold [`Str0mNetChannel::send_frame`] returns
/// [`Str0mNetError::Backpressure`](Self::Backpressure) so callers apply flow
/// control rather than growing the queue without bound (#lzstr0mframe).
const MAX_PENDING_FRAMES: usize = 1024;

/// Maximum number of inbound frames buffered for the consumer in
/// [`Shared::inbox`]. Above this threshold the oldest frame is dropped (and
/// counted in `dropped_inbox_frames`) so a slow consumer cannot grow receive
/// memory without bound (#lzstr0mnetinbox). str0m's SCTP stack still owns
/// wire-level reliability; this cap is the local memory safety valve.
const MAX_INBOX_FRAMES: usize = 1024;

/// State shared between the public handles and the driver thread.
struct Shared {
    inbox: Mutex<VecDeque<Vec<u8>>>,
    open: AtomicBool,
    closed: AtomicBool,
    /// Number of frames currently buffered in the driver's outbound queue
    /// (`DriverCmd::Send` messages not yet accepted by `Channel::write`). Used
    /// by [`Str0mNetChannel::send_frame`] to apply backpressure before enqueue.
    pending_frames: AtomicUsize,
    /// Inbound frames dropped because `inbox` reached `MAX_INBOX_FRAMES` while
    /// the consumer was not draining (`try_recv_frame`). Monotonically
    /// increasing; read via [`Str0mNet::dropped_inbox_frames`].
    dropped_inbox_frames: AtomicUsize,
    /// Last driver-side failure (e.g. an SDP answer that parses but fails
    /// `accept_answer` application — ICE/DTLS fingerprint or mid mismatch).
    /// Surfaces a cause when [`Str0mNet::wait_open`] times out instead of the
    /// previously silent hang (#lzstr0mnetacceptanswer).
    last_error: Mutex<Option<String>>,
}

/// A networked str0m DataChannel peer.
///
/// Construct with [`Str0mNet::offer`] (offerer) or [`Str0mNet::answer`]
/// (answerer), complete the handshake with [`accept_answer`](Self::accept_answer)
/// / [`add_remote_candidate`](Self::add_remote_candidate), then obtain a
/// [`DataChannel`] with [`channel`](Self::channel).
pub struct Str0mNet {
    cmd_tx: Sender<DriverCmd>,
    shared: Arc<Shared>,
    local_candidate: String,
    driver: Option<JoinHandle<()>>,
    is_offerer: bool,
}

impl Str0mNet {
    /// Offerer: bind a UDP socket, open the `lazily-ipc` data channel, and
    /// produce the SDP **offer** to send to the peer via signaling. Apply the
    /// peer's answer with [`accept_answer`](Self::accept_answer).
    pub fn offer(bind: SocketAddr) -> Result<(Self, String), Str0mNetError> {
        let socket = UdpSocket::bind(bind)?;
        let local_addr = socket.local_addr()?;
        let now = Instant::now();
        let mut rtc = Rtc::new(now);

        let mut api = rtc.sdp_api();
        let cid = api.add_channel("lazily-ipc".to_string());
        let (offer, pending) = api
            .apply()
            .ok_or_else(|| Str0mNetError::Sdp("str0m produced no offer".into()))?;

        // Trickle ICE: add our host candidate *after* the SDP is created, so the
        // SDP is candidate-free and the candidate is exchanged out of band.
        let local = host_candidate(local_addr)?;
        rtc.add_local_candidate(local.clone());

        let net = Self::spawn(
            rtc,
            Some(pending),
            Some(cid),
            socket,
            local_addr,
            local,
            true,
        );
        Ok((net, offer.to_sdp_string()))
    }

    /// Answerer: bind a UDP socket, accept the peer's SDP **offer**, and produce
    /// the SDP **answer** to send back via signaling.
    pub fn answer(bind: SocketAddr, offer_sdp: &str) -> Result<(Self, String), Str0mNetError> {
        let socket = UdpSocket::bind(bind)?;
        let local_addr = socket.local_addr()?;
        let now = Instant::now();
        let mut rtc = Rtc::new(now);

        let offer =
            SdpOffer::from_sdp_string(offer_sdp).map_err(|e| Str0mNetError::Sdp(e.to_string()))?;
        let answer = rtc
            .sdp_api()
            .accept_offer(offer)
            .map_err(|e| Str0mNetError::Sdp(e.to_string()))?;

        let local = host_candidate(local_addr)?;
        rtc.add_local_candidate(local.clone());

        let net = Self::spawn(rtc, None, None, socket, local_addr, local, false);
        Ok((net, answer.to_sdp_string()))
    }

    fn spawn(
        rtc: Rtc,
        pending: Option<SdpPendingOffer>,
        cid: Option<ChannelId>,
        socket: UdpSocket,
        local_addr: SocketAddr,
        local_candidate: Candidate,
        is_offerer: bool,
    ) -> Self {
        let shared = Arc::new(Shared {
            inbox: Mutex::new(VecDeque::new()),
            open: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            pending_frames: AtomicUsize::new(0),
            dropped_inbox_frames: AtomicUsize::new(0),
            last_error: Mutex::new(None),
        });
        let (cmd_tx, cmd_rx) = channel();
        let candidate_string = local_candidate.to_sdp_string();

        let driver_shared = shared.clone();
        let driver = std::thread::Builder::new()
            .name("str0m-net-driver".to_string())
            .spawn(move || {
                run_driver(rtc, pending, cid, socket, local_addr, cmd_rx, driver_shared);
            })
            .expect("spawn str0m-net driver thread");

        Self {
            cmd_tx,
            shared,
            local_candidate: candidate_string,
            driver: Some(driver),
            is_offerer,
        }
    }

    /// This peer's host ICE candidate, as an SDP `candidate:` string. Send it to
    /// the remote peer (e.g. via [`SignalingClient::ice`](crate::SignalingClient)),
    /// which feeds it to [`add_remote_candidate`](Self::add_remote_candidate).
    pub fn local_candidate(&self) -> &str {
        &self.local_candidate
    }

    /// Apply the peer's SDP **answer** (offerer only).
    pub fn accept_answer(&self, answer_sdp: &str) -> Result<(), Str0mNetError> {
        if !self.is_offerer {
            return Err(Str0mNetError::NotOfferer);
        }
        // Validate the SDP eagerly so a bad answer surfaces here, not silently in
        // the driver thread.
        SdpAnswer::from_sdp_string(answer_sdp).map_err(|e| Str0mNetError::Sdp(e.to_string()))?;
        self.cmd_tx
            .send(DriverCmd::AcceptAnswer(answer_sdp.to_string()))
            .map_err(|_| Str0mNetError::Closed)
    }

    /// Feed a trickled remote ICE candidate (the peer's
    /// [`local_candidate`](Self::local_candidate)).
    pub fn add_remote_candidate(&self, candidate_sdp: &str) -> Result<(), Str0mNetError> {
        Candidate::from_sdp_string(candidate_sdp).map_err(|e| Str0mNetError::Sdp(e.to_string()))?;
        self.cmd_tx
            .send(DriverCmd::AddRemoteCandidate(candidate_sdp.to_string()))
            .map_err(|_| Str0mNetError::Closed)
    }

    /// Whether the data channel is currently open.
    pub fn is_open(&self) -> bool {
        self.shared.open.load(Ordering::SeqCst) && !self.shared.closed.load(Ordering::SeqCst)
    }

    /// Number of inbound frames dropped because the receive buffer was
    /// saturated (`MAX_INBOX_FRAMES`). Non-zero indicates the consumer is not
    /// calling `try_recv_frame` fast enough; sustained drops warrant a Snapshot
    /// resync (#lzstr0mnetinbox).
    pub fn dropped_inbox_frames(&self) -> usize {
        self.shared.dropped_inbox_frames.load(Ordering::Relaxed)
    }

    /// Last driver-side failure (e.g. an SDP answer that parses but fails
    /// `accept_answer` application — ICE/DTLS fingerprint or mid mismatch).
    /// Returns the most recent apply-time failure so a `wait_open` timeout
    /// has a diagnosable cause instead of a silent hang (#lzstr0mnetacceptanswer).
    pub fn last_error(&self) -> Option<String> {
        self.shared.last_error.lock().clone()
    }

    /// Block until the data channel opens or `timeout` elapses. Returns whether
    /// it opened.
    pub fn wait_open(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if self.is_open() {
                return true;
            }
            if self.shared.closed.load(Ordering::SeqCst) {
                return false;
            }
            if Instant::now() >= deadline {
                return self.is_open();
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// A cloneable [`DataChannel`] handle for the
    /// [`WebRtcSink`](crate::WebRtcSink) / [`WebRtcSource`](crate::WebRtcSource)
    /// bridge.
    pub fn channel(&self) -> Str0mNetChannel {
        Str0mNetChannel {
            cmd_tx: self.cmd_tx.clone(),
            shared: self.shared.clone(),
        }
    }
}

impl Drop for Str0mNet {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(DriverCmd::Shutdown);
        if let Some(driver) = self.driver.take() {
            let _ = driver.join();
        }
    }
}

/// One end of a [`Str0mNet`] connection, usable as a [`DataChannel`].
#[derive(Clone)]
pub struct Str0mNetChannel {
    cmd_tx: Sender<DriverCmd>,
    shared: Arc<Shared>,
}

impl DataChannel for Str0mNetChannel {
    type Error = Str0mNetError;

    fn send_frame(&self, frame: Vec<u8>) -> Result<(), Self::Error> {
        if self.shared.closed.load(Ordering::SeqCst) {
            return Err(Str0mNetError::Closed);
        }
        // Backpressure gate: reject the enqueue once the driver's outbound
        // queue is at capacity. Without this check a caller producing frames
        // faster than the SCTP drain would grow `out_pending` without bound,
        // and (pre-#lzstr0mframe) the driver's flush loop would silently drop
        // any frame that hit the `Ok(false)` backpressure path.
        if self.shared.pending_frames.load(Ordering::Relaxed) >= MAX_PENDING_FRAMES {
            return Err(Str0mNetError::Backpressure);
        }
        self.cmd_tx
            .send(DriverCmd::Send(frame))
            .map_err(|_| Str0mNetError::Closed)
    }

    fn try_recv_frame(&self) -> Result<Option<Vec<u8>>, Self::Error> {
        if let Some(frame) = self.shared.inbox.lock().pop_front() {
            return Ok(Some(frame));
        }
        if self.shared.closed.load(Ordering::SeqCst) && !self.is_open() {
            // Surface closure only once the inbox is drained, so frames buffered
            // before a clean shutdown are still delivered.
            return Err(Str0mNetError::Closed);
        }
        Ok(None)
    }

    fn is_open(&self) -> bool {
        self.shared.open.load(Ordering::SeqCst) && !self.shared.closed.load(Ordering::SeqCst)
    }
}

/// Build the single host ICE candidate for our bound UDP address.
fn host_candidate(addr: SocketAddr) -> Result<Candidate, Str0mNetError> {
    Candidate::host(addr, "udp").map_err(|e| Str0mNetError::Sdp(e.to_string()))
}

/// The driver loop. Owns the `Rtc` and the UDP socket; pumps the sans-IO state
/// machine over real I/O until shutdown or the connection dies.
fn run_driver(
    mut rtc: Rtc,
    mut pending: Option<SdpPendingOffer>,
    mut cid: Option<ChannelId>,
    socket: UdpSocket,
    local_addr: SocketAddr,
    cmd_rx: Receiver<DriverCmd>,
    shared: Arc<Shared>,
) {
    let mut buf = [0u8; 2048];
    // Frames requested before the channel opened; flushed once it is.
    let mut out_pending: VecDeque<Vec<u8>> = VecDeque::new();

    'outer: loop {
        // 1. Apply control commands.
        loop {
            match cmd_rx.try_recv() {
                Ok(DriverCmd::Shutdown) => break 'outer,
                Ok(DriverCmd::AcceptAnswer(sdp)) => {
                    if let (Some(p), Ok(answer)) =
                        (pending.take(), SdpAnswer::from_sdp_string(&sdp))
                        && let Err(e) = rtc.sdp_api().accept_answer(p, answer)
                    {
                        *shared.last_error.lock() =
                            Some(format!("accept_answer apply failed: {e}"));
                    }
                }
                Ok(DriverCmd::AddRemoteCandidate(s)) => {
                    if let Ok(c) = Candidate::from_sdp_string(&s) {
                        rtc.add_remote_candidate(c);
                    }
                }
                Ok(DriverCmd::Send(bytes)) => {
                    out_pending.push_back(bytes);
                    shared.pending_frames.fetch_add(1, Ordering::Relaxed);
                }
                Err(TryRecvError::Empty) => break,
                // All handles dropped without an explicit Shutdown.
                Err(TryRecvError::Disconnected) => break 'outer,
            }
        }

        // 2. Flush buffered outbound frames once the channel is open.
        if shared.open.load(Ordering::SeqCst)
            && let Some(id) = cid
        {
            while let Some(frame) = out_pending.pop_front() {
                match rtc.channel(id) {
                    Some(mut ch) => match ch.write(true, &frame) {
                        // Accepted by the SCTP layer: the frame will be
                        // transmitted in order. Decrement the backpressure
                        // counter so `send_frame` can enqueue again.
                        Ok(true) => {
                            shared.pending_frames.fetch_sub(1, Ordering::Relaxed);
                        }
                        // Ok(false) = SCTP send buffer full (backpressure).
                        // Err     = channel broken / proto error.
                        //
                        // In BOTH cases the frame must be re-queued, not
                        // dropped. Pre-#lzstr0mframe this branch was a bare
                        // `if ch.write(...).is_err() { break; }` which (a)
                        // ignored the `Ok(false)` backpressure signal entirely
                        // — silently dropping every frame that exceeded the
                        // SCTP send window — and (b) dropped the frame on `Err`
                        // too. Re-queue + yield lets the driver advance
                        // `poll_output` / `recv_from` to drain the buffer (for
                        // `Ok(false)`) or detect a dead `Rtc` on the next
                        // `is_alive()` check (for `Err`).
                        Ok(false) | Err(_) => {
                            out_pending.push_front(frame);
                            break;
                        }
                    },
                    None => {
                        out_pending.push_front(frame);
                        break;
                    }
                }
            }
        }

        if !rtc.is_alive() {
            break;
        }

        // 3. Drain outputs; the loop ends on the next timeout deadline.
        let timeout = loop {
            match rtc.poll_output() {
                Ok(Output::Timeout(t)) => break t,
                Ok(Output::Transmit(t)) => {
                    // Surface send failures instead of silently dropping the
                    // ICE/DTLS/SCTP packet (#lzstr0mpolldrive). Pre-fix this
                    // was `let _ = socket.send_to(...)` which discarded EVERY
                    // error: ENOBUFS under send-buffer pressure, ICMP-driven
                    // ConnectionRefused (peer unreachable), ENETUNREACH/
                    // EHOSTUNREACH (route flap), EBADF (socket closed) — all
                    // silently lost packets and the handshake / data path
                    // just stalled without diagnostics.
                    //
                    // `WouldBlock`/`Interrupted` are retryable on a blocking
                    // socket (transient send-buffer full / signal); str0m will
                    // re-emit the Transmit on a later `poll_output`, so just
                    // continue the drain loop. Any other error indicates a
                    // fatal socket/route condition that re-signaling must
                    // rebuild from — break the driver.
                    match socket.send_to(&t.contents, t.destination) {
                        Ok(_) => {}
                        Err(e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                || e.kind() == std::io::ErrorKind::Interrupted =>
                        {
                            continue;
                        }
                        Err(_) => break 'outer,
                    }
                }
                Ok(Output::Event(e)) => handle_event(e, &mut cid, &shared),
                Err(_) => break 'outer,
            }
        };

        // 4. Wait for inbound datagrams. The cap is a *command-poll interval*,
        //    not a str0m timing parameter: control commands (Send /
        //    AcceptAnswer / AddRemoteCandidate / Shutdown) are read from
        //    `cmd_rx` at the top of each `'outer` iteration, so capping the
        //    recv wait keeps their latency bounded. str0m itself is fed an
        //    accurate time advance via `Input::Timeout(now)` whenever the
        //    socket times out without data; calling that "early" (every
        //    COMMAND_POLL_INTERVAL during idle) is harmless — str0m just
        //    re-emits its pending deadline if it isn't time yet.
        const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(15);
        let now = Instant::now();
        let wait = timeout
            .checked_duration_since(now)
            .unwrap_or(Duration::ZERO)
            .min(COMMAND_POLL_INTERVAL)
            .max(Duration::from_millis(1));
        if socket.set_read_timeout(Some(wait)).is_err() {
            break;
        }
        match socket.recv_from(&mut buf) {
            Ok((n, src)) => {
                if let Ok(contents) = buf[..n].try_into() {
                    let _ = rtc.handle_input(Input::Receive(
                        Instant::now(),
                        Receive {
                            proto: Protocol::Udp,
                            source: src,
                            destination: local_addr,
                            contents,
                        },
                    ));
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                let _ = rtc.handle_input(Input::Timeout(Instant::now()));
            }
            Err(_) => break,
        }
    }

    // The handshake will never complete after this point; mark closed so the
    // sync sink/source surface `Err`. Reset the backpressure counter too: any
    // remaining queued frames will never be delivered, so `send_frame` should
    // surface `Closed` (via the cmd_tx drop / closed atomic) rather than
    // `Backpressure` forever.
    shared.open.store(false, Ordering::SeqCst);
    shared.closed.store(true, Ordering::SeqCst);
    shared.pending_frames.store(0, Ordering::SeqCst);
}

fn push_inbox_frame(shared: &Shared, frame: Vec<u8>) {
    let mut inbox = shared.inbox.lock();
    if inbox.len() >= MAX_INBOX_FRAMES {
        inbox.pop_front();
        shared.dropped_inbox_frames.fetch_add(1, Ordering::Relaxed);
    }
    inbox.push_back(frame);
}

fn handle_event(event: Event, cid: &mut Option<ChannelId>, shared: &Shared) {
    match event {
        Event::ChannelOpen(id, _label) => {
            *cid = Some(id);
            shared.open.store(true, Ordering::SeqCst);
        }
        Event::ChannelData(data) => {
            push_inbox_frame(shared, data.data);
        }
        Event::ChannelClose(_) => {
            shared.open.store(false, Ordering::SeqCst);
            shared.closed.store(true, Ordering::SeqCst);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_shared() -> Shared {
        Shared {
            inbox: Mutex::new(VecDeque::new()),
            open: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            pending_frames: AtomicUsize::new(0),
            dropped_inbox_frames: AtomicUsize::new(0),
            last_error: Mutex::new(None),
        }
    }

    #[test]
    fn last_error_round_trips_apply_failure() {
        let shared = make_shared();
        assert!(
            shared.last_error.lock().is_none(),
            "last_error starts clear on a fresh peer"
        );
        *shared.last_error.lock() = Some("fingerprint mismatch".to_string());
        assert_eq!(
            shared.last_error.lock().clone(),
            Some("fingerprint mismatch".to_string()),
            "apply-time failure must be stored and readable for wait_open diagnosis"
        );
    }

    #[test]
    fn inbox_caps_at_max_inbox_frames_and_counts_drops() {
        let shared = make_shared();
        for _ in 0..(MAX_INBOX_FRAMES + 50) {
            push_inbox_frame(&shared, vec![0u8]);
        }
        {
            let inbox = shared.inbox.lock();
            assert_eq!(
                inbox.len(),
                MAX_INBOX_FRAMES,
                "inbox must be capped at MAX_INBOX_FRAMES"
            );
            assert_eq!(
                shared.dropped_inbox_frames.load(Ordering::Relaxed),
                50,
                "overflowing frames must be counted as dropped"
            );
        }
        push_inbox_frame(&shared, vec![42u8]);
        {
            let inbox = shared.inbox.lock();
            assert_eq!(
                inbox.len(),
                MAX_INBOX_FRAMES,
                "capped inbox must not grow further"
            );
            assert_eq!(
                inbox.back().map(|v| v[0]),
                Some(42u8),
                "drop-oldest ring semantics must retain the newest frame"
            );
            assert_eq!(
                shared.dropped_inbox_frames.load(Ordering::Relaxed),
                51,
                "one more drop to make room for the new frame"
            );
        }
    }
}
