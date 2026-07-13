//! Phase 4 of the RelayCell backpressure plan — the `Transport` seam.
//!
//! See `lazily-spec/docs/relaycell.md` §5 and
//! `lazily-spec/docs/relaycell-backpressure-analysis.md` §4.6. `Transport`
//! abstracts ingress/egress delivery so the mechanism is pluggable and
//! per-binding: `InProc` (direct), `CrossThread` (native mpsc / shared context),
//! `IpcTransport`, `WsTransport`. A `RelayCell` is written once against
//! `Transport`; **the merge algebra — not the transport — guarantees converged
//! state**, so transports may differ across bindings and still converge (the
//! `LazilyFormal.Relay.transport_independent` invariant).
//!
//! This reference file models the two ends of the spectrum — direct delivery and
//! a *framed* transport (network-style batching / MTU chunking) — enough to prove
//! that relay egress is invariant across transport framing.

use std::collections::VecDeque;

/// A pluggable delivery mechanism for relay ops. `deliver` enqueues; `poll`
/// pulls the next transport-defined frame (a batch of ready ops). Framing is the
/// transport's business; the relay merges whatever each frame delivers.
pub trait Transport<T> {
    /// Enqueue an op for delivery.
    fn deliver(&mut self, op: T);
    /// Pull the next ready frame (empty when nothing is ready). Ops are returned
    /// in delivery order within a frame.
    fn poll(&mut self) -> Vec<T>;
    /// Whether any op is still buffered for delivery.
    fn has_pending(&self) -> bool;
}

/// `InProc` — direct delivery: every buffered op is handed over in one frame.
pub struct InProcTransport<T> {
    buf: VecDeque<T>,
}

impl<T> Default for InProcTransport<T> {
    fn default() -> Self {
        Self {
            buf: VecDeque::new(),
        }
    }
}

impl<T> InProcTransport<T> {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<T> Transport<T> for InProcTransport<T> {
    fn deliver(&mut self, op: T) {
        self.buf.push_back(op);
    }
    fn poll(&mut self) -> Vec<T> {
        self.buf.drain(..).collect()
    }
    fn has_pending(&self) -> bool {
        !self.buf.is_empty()
    }
}

/// A *framed* transport — models `CrossThread`/`Ipc`/`Ws`: ops are delivered in
/// bounded frames of at most `frame_size` (an MTU / batch boundary). Different
/// `frame_size`s are different framings of the same op stream.
pub struct FramedTransport<T> {
    buf: VecDeque<T>,
    frame_size: usize,
}

impl<T> FramedTransport<T> {
    pub fn new(frame_size: usize) -> Self {
        Self {
            buf: VecDeque::new(),
            frame_size: frame_size.max(1),
        }
    }
}

impl<T> Transport<T> for FramedTransport<T> {
    fn deliver(&mut self, op: T) {
        self.buf.push_back(op);
    }
    fn poll(&mut self) -> Vec<T> {
        let n = self.frame_size.min(self.buf.len());
        self.buf.drain(..n).collect()
    }
    fn has_pending(&self) -> bool {
        !self.buf.is_empty()
    }
}
