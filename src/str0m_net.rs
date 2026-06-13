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
use std::sync::atomic::{AtomicBool, Ordering};
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

/// State shared between the public handles and the driver thread.
struct Shared {
    inbox: Mutex<VecDeque<Vec<u8>>>,
    open: AtomicBool,
    closed: AtomicBool,
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
                    {
                        let _ = rtc.sdp_api().accept_answer(p, answer);
                    }
                }
                Ok(DriverCmd::AddRemoteCandidate(s)) => {
                    if let Ok(c) = Candidate::from_sdp_string(&s) {
                        rtc.add_remote_candidate(c);
                    }
                }
                Ok(DriverCmd::Send(bytes)) => out_pending.push_back(bytes),
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
                    Some(mut ch) => {
                        if ch.write(true, &frame).is_err() {
                            break;
                        }
                    }
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
                    let _ = socket.send_to(&t.contents, t.destination);
                }
                Ok(Output::Event(e)) => handle_event(e, &mut cid, &shared),
                Err(_) => break 'outer,
            }
        };

        // 4. Wait for inbound datagrams up to the deadline, capped so control
        //    commands are still serviced promptly.
        let now = Instant::now();
        let wait = timeout
            .checked_duration_since(now)
            .unwrap_or(Duration::ZERO)
            .min(Duration::from_millis(15))
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
    // sync sink/source surface `Err`.
    shared.open.store(false, Ordering::SeqCst);
    shared.closed.store(true, Ordering::SeqCst);
}

fn handle_event(event: Event, cid: &mut Option<ChannelId>, shared: &Shared) {
    match event {
        Event::ChannelOpen(id, _label) => {
            *cid = Some(id);
            shared.open.store(true, Ordering::SeqCst);
        }
        Event::ChannelData(data) => {
            shared.inbox.lock().push_back(data.data);
        }
        Event::ChannelClose(_) => {
            shared.open.store(false, Ordering::SeqCst);
            shared.closed.store(true, Ordering::SeqCst);
        }
        _ => {}
    }
}
