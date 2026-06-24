//! Concrete sans-IO **str0m** DataChannel backend for the WebRTC transport
//! (#webrtcbackend / #jpf1), behind the `webrtc-str0m` feature.
//!
//! str0m is sans-IO: it never touches sockets or threads itself — the caller
//! drives it by draining [`Rtc::poll_output`] and feeding [`Rtc::handle_input`].
//! That property is exactly what makes a real DataChannel handshake
//! **deterministically loopback-testable in-process**: [`Str0mLoopback`] owns two
//! `Rtc` instances and routes each side's `Output::Transmit` into the other's
//! `Input::Receive`, advancing a synthetic clock, until ICE/DTLS/SCTP complete
//! and the SCTP data channel opens. Each side is then exposed as a
//! [`DataChannel`](crate::DataChannel) so the `WebRtcSink`/`WebRtcSource` bridge
//! (permission filtering, `IpcMessage` codec) runs over a genuine str0m channel.
//!
//! The networked (non-loopback) backend [`Str0mNet`](crate::Str0mNet) reuses the
//! same pump loop with a real UDP socket and a background driver thread instead
//! of the in-memory packet route, exchanging the SDP/ICE handshake over
//! [`SignalingClient`](crate::SignalingClient). It needs live connectivity so it
//! cannot run on the synthetic clock; see `src/str0m_net.rs` and
//! `tests/str0m_net.rs` (real two-socket `127.0.0.1` round trip).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::rc::Rc;
use std::time::{Duration, Instant};

use str0m::channel::ChannelId;
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, Input, Output, Rtc};

use crate::webrtc_transport::DataChannel;

/// Which end of a [`Str0mLoopback`] pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The offerer.
    Left,
    /// The answerer.
    Right,
}

/// Error from the str0m loopback backend.
#[derive(Debug)]
pub enum Str0mError {
    /// A str0m API call failed.
    Rtc(str0m::RtcError),
    /// The handshake did not complete within the pump budget.
    HandshakeTimeout,
    /// SDP offer/answer negotiation failed.
    Negotiation(String),
}

impl std::fmt::Display for Str0mError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rtc(e) => write!(f, "str0m rtc error: {e}"),
            Self::HandshakeTimeout => write!(f, "str0m loopback handshake did not converge"),
            Self::Negotiation(e) => write!(f, "str0m sdp negotiation failed: {e}"),
        }
    }
}

impl std::error::Error for Str0mError {}

impl From<str0m::RtcError> for Str0mError {
    fn from(e: str0m::RtcError) -> Self {
        Self::Rtc(e)
    }
}

impl From<str0m::error::IceError> for Str0mError {
    fn from(e: str0m::error::IceError) -> Self {
        Self::Negotiation(e.to_string())
    }
}

struct Peer {
    rtc: Rtc,
    addr: SocketAddr,
    cid: Option<ChannelId>,
    open: bool,
    inbox: VecDeque<Vec<u8>>,
    dropped_inbox_frames: usize,
}

/// Maximum inbound frames buffered for one loopback endpoint at once. Above this
/// threshold the oldest frame is dropped, matching the network str0m inbox cap
/// so deterministic loopback tests cannot grow memory without bound
/// (#lzstr0mloopbackinbox).
const MAX_INBOX_FRAMES: usize = 1024;

impl Peer {
    fn new(addr: SocketAddr, now: Instant) -> Result<Self, Str0mError> {
        let mut rtc = Rtc::new(now);
        rtc.add_local_candidate(Candidate::host(addr, "udp").map_err(Str0mError::from)?);
        Ok(Self {
            rtc,
            addr,
            cid: None,
            open: false,
            inbox: VecDeque::new(),
            dropped_inbox_frames: 0,
        })
    }
}

struct LoopbackState {
    left: Peer,
    right: Peer,
    now: Instant,
}

impl LoopbackState {
    /// Drain both peers to quiescence, routing transmits and advancing the clock,
    /// for up to `max_steps` iterations. Returns once nothing is left to do or the
    /// budget is exhausted.
    fn pump(&mut self, max_steps: usize) {
        // (side a packets feed side b and vice versa)
        for _ in 0..max_steps {
            let mut progressed = false;
            // Drain each peer's outputs; queue transmits for the *other* peer.
            let mut to_left: Vec<Vec<u8>> = Vec::new();
            let mut to_right: Vec<Vec<u8>> = Vec::new();

            for side in [Side::Left, Side::Right] {
                loop {
                    let out = {
                        let peer = self.peer_mut(side);
                        match peer.rtc.poll_output() {
                            Ok(o) => o,
                            Err(_) => break,
                        }
                    };
                    match out {
                        Output::Timeout(_) => break,
                        Output::Transmit(t) => {
                            progressed = true;
                            let bytes = t.contents.to_vec();
                            match side {
                                Side::Left => to_right.push(bytes),
                                Side::Right => to_left.push(bytes),
                            }
                        }
                        Output::Event(e) => {
                            progressed = true;
                            self.handle_event(side, e);
                        }
                    }
                }
            }

            // Deliver routed packets as Input::Receive on the destination peer.
            let now = self.now;
            for bytes in to_left {
                let (src, dst) = (self.right.addr, self.left.addr);
                if Self::deliver(&mut self.left.rtc, now, src, dst, &bytes) {
                    progressed = true;
                }
            }
            for bytes in to_right {
                let (src, dst) = (self.left.addr, self.right.addr);
                if Self::deliver(&mut self.right.rtc, now, src, dst, &bytes) {
                    progressed = true;
                }
            }

            if !progressed {
                // Advance the clock to the nearest requested deadline to let
                // DTLS/SCTP/ICE timers fire, then continue.
                self.now += Duration::from_millis(5);
                let now = self.now;
                let _ = self.left.rtc.handle_input(Input::Timeout(now));
                let _ = self.right.rtc.handle_input(Input::Timeout(now));
                if self.left.open && self.right.open {
                    break;
                }
            }
        }
    }

    fn deliver(
        rtc: &mut Rtc,
        now: Instant,
        src: SocketAddr,
        dst: SocketAddr,
        bytes: &[u8],
    ) -> bool {
        let contents = match bytes.try_into() {
            Ok(c) => c,
            Err(_) => return false,
        };
        rtc.handle_input(Input::Receive(
            now,
            Receive {
                proto: Protocol::Udp,
                source: src,
                destination: dst,
                contents,
            },
        ))
        .is_ok()
    }

    fn handle_event(&mut self, side: Side, event: Event) {
        match event {
            Event::ChannelOpen(cid, _) => {
                let peer = self.peer_mut(side);
                peer.cid = Some(cid);
                peer.open = true;
            }
            Event::ChannelData(data) => {
                push_inbox_frame(self.peer_mut(side), data.data);
            }
            _ => {}
        }
    }

    fn peer_mut(&mut self, side: Side) -> &mut Peer {
        match side {
            Side::Left => &mut self.left,
            Side::Right => &mut self.right,
        }
    }

    fn peer(&self, side: Side) -> &Peer {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }
}

fn push_inbox_frame(peer: &mut Peer, frame: Vec<u8>) {
    if peer.inbox.len() >= MAX_INBOX_FRAMES {
        peer.inbox.pop_front();
        peer.dropped_inbox_frames += 1;
    }
    peer.inbox.push_back(frame);
}

/// In-process loopback of two str0m peers connected by a real DataChannel.
///
/// Construct with [`Str0mLoopback::connect`]; obtain the two ends with
/// [`Str0mLoopback::endpoint`] and wrap them in
/// [`WebRtcSink`](crate::WebRtcSink) / [`WebRtcSource`](crate::WebRtcSource).
#[derive(Clone)]
pub struct Str0mLoopback {
    state: Rc<RefCell<LoopbackState>>,
}

impl Str0mLoopback {
    /// Negotiate two peers over a synthetic in-memory transport and drive the
    /// handshake until the data channel opens (or the pump budget is exhausted).
    pub fn connect() -> Result<Self, Str0mError> {
        let now = Instant::now();
        let left_addr: SocketAddr = "127.0.0.1:4000".parse().unwrap();
        let right_addr: SocketAddr = "127.0.0.1:4001".parse().unwrap();

        let mut left = Peer::new(left_addr, now)?;
        let right = Peer::new(right_addr, now)?;

        // Offerer adds the data channel and creates the offer.
        let mut change = left.rtc.sdp_api();
        let cid = change.add_channel("lazily-ipc".to_string());
        left.cid = Some(cid);
        let (offer, pending) = change
            .apply()
            .ok_or_else(|| Str0mError::Negotiation("no offer produced".into()))?;

        let mut state = LoopbackState { left, right, now };

        // Answerer accepts and produces the answer.
        let answer = state
            .right
            .rtc
            .sdp_api()
            .accept_offer(offer)
            .map_err(|e| Str0mError::Negotiation(e.to_string()))?;
        state
            .left
            .rtc
            .sdp_api()
            .accept_answer(pending, answer)
            .map_err(|e| Str0mError::Negotiation(e.to_string()))?;

        state.pump(20_000);

        if !(state.left.open && state.right.open) {
            return Err(Str0mError::HandshakeTimeout);
        }

        Ok(Self {
            state: Rc::new(RefCell::new(state)),
        })
    }

    /// A [`DataChannel`] handle for one end of the loopback.
    pub fn endpoint(&self, side: Side) -> Str0mChannel {
        Str0mChannel {
            state: self.state.clone(),
            side,
        }
    }
}

/// One end of a [`Str0mLoopback`], usable as a [`DataChannel`].
///
/// Because str0m is sans-IO and the loopback is single-threaded, each call pumps
/// the shared driver so frames actually cross the channel.
#[derive(Clone)]
pub struct Str0mChannel {
    state: Rc<RefCell<LoopbackState>>,
    side: Side,
}

impl Str0mChannel {
    /// Number of inbound loopback frames dropped because this endpoint's receive
    /// buffer reached `MAX_INBOX_FRAMES`. Non-zero means the consumer is not
    /// draining [`DataChannel::try_recv_frame`] fast enough and should resync or
    /// assert on the loss.
    pub fn dropped_inbox_frames(&self) -> usize {
        self.state.borrow().peer(self.side).dropped_inbox_frames
    }
}

impl DataChannel for Str0mChannel {
    type Error = Str0mError;

    fn send_frame(&self, frame: Vec<u8>) -> Result<(), Self::Error> {
        {
            let mut state = self.state.borrow_mut();
            let cid = state
                .peer(self.side)
                .cid
                .ok_or(Str0mError::HandshakeTimeout)?;
            let mut channel = state
                .peer_mut(self.side)
                .rtc
                .channel(cid)
                .ok_or(Str0mError::HandshakeTimeout)?;
            channel.write(true, &frame).map_err(Str0mError::from)?;
        }
        self.state.borrow_mut().pump(2_000);
        Ok(())
    }

    fn try_recv_frame(&self) -> Result<Option<Vec<u8>>, Self::Error> {
        self.state.borrow_mut().pump(2_000);
        Ok(self
            .state
            .borrow_mut()
            .peer_mut(self.side)
            .inbox
            .pop_front())
    }

    fn is_open(&self) -> bool {
        self.state.borrow().peer(self.side).open
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::webrtc_transport::{WebRtcSink, WebRtcSource};
    use crate::{
        IpcMessage, IpcSink, IpcSource, NodeId, NodeSnapshot, OpKind, PeerId, PeerPermissions,
        Snapshot,
    };

    #[test]
    fn loopback_inbox_caps_at_capacity_and_counts_drops() {
        let link = Str0mLoopback::connect().expect("str0m loopback handshake to converge");
        let channel = link.endpoint(Side::Right);

        {
            let mut state = link.state.borrow_mut();
            let peer = state.peer_mut(Side::Right);
            for i in 0..(MAX_INBOX_FRAMES + 50) {
                push_inbox_frame(peer, i.to_le_bytes().to_vec());
            }
        }

        let first_kept = 50usize.to_le_bytes().to_vec();
        let last_kept = (MAX_INBOX_FRAMES + 50 - 1).to_le_bytes().to_vec();
        let state = link.state.borrow();
        let peer = state.peer(Side::Right);
        assert_eq!(
            peer.inbox.len(),
            MAX_INBOX_FRAMES,
            "loopback inbox must be capped at MAX_INBOX_FRAMES"
        );
        assert_eq!(peer.inbox.front(), Some(&first_kept));
        assert_eq!(peer.inbox.back(), Some(&last_kept));
        drop(state);
        assert_eq!(channel.dropped_inbox_frames(), 50);
    }

    #[test]
    fn loopback_datachannel_carries_a_snapshot() {
        // The pump advances a synthetic clock deterministically (no wall-clock
        // dependency), so the ICE/DTLS/SCTP handshake converging is a hard
        // requirement — a regression that breaks it must fail, not silently skip.
        let link = Str0mLoopback::connect().expect("str0m loopback handshake to converge");

        let peer = PeerId(1);
        let mut perms = PeerPermissions::new();
        perms.allow_many(peer, OpKind::Read, [NodeId(1), NodeId(2)]);

        let mut sink = WebRtcSink::new(link.endpoint(Side::Left), perms, peer);
        let mut source = WebRtcSource::new(link.endpoint(Side::Right));

        let snapshot = Snapshot::new(
            1,
            vec![
                NodeSnapshot::payload(NodeId(1), "t", vec![1, 2, 3]),
                NodeSnapshot::payload(NodeId(2), "t", vec![4, 5, 6]),
            ],
            vec![],
            vec![NodeId(1), NodeId(2)],
        );
        sink.send(&IpcMessage::Snapshot(snapshot)).unwrap();

        // Pull until the frame arrives (a few pump rounds), bounded.
        let mut got = None;
        for _ in 0..50 {
            if let Some(msg) = source.recv().unwrap() {
                got = Some(msg);
                break;
            }
        }
        match got.expect("snapshot to arrive over the str0m data channel") {
            IpcMessage::Snapshot(s) => {
                let ids: Vec<u64> = s.nodes.iter().map(|n| n.node.0).collect();
                assert_eq!(ids, vec![1, 2]);
            }
            other => panic!("expected snapshot, got {other:?}"),
        }
    }
}
