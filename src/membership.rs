//! Phase 2 of the realtime + distributed primitives plan — `#lzmemb`
//! membership + failure detection.
//!
//! See `lazily-spec/docs/membership.md` and the formal model
//! `lazily-formal/LazilyFormal/Membership.lean`. A [`MembershipCell`] is a
//! reactive view of the live peer set, backed by SWIM-style heartbeats + a
//! **Phi-accrual** failure detector. Per-peer state is `Alive | Suspect | Dead |
//! Left`; the derived [`PeerSet`](MembershipCell::peer_set) is the `Alive` peers.
//!
//! The pure compute **core** ([`MembershipCore`] + [`PhiAccrual`]) is the
//! Phi-accrual math + SWIM state machine over plain state (C++-eligible,
//! `BytesPayload`); the reactive cell projects the alive set onto a `Cell` so
//! `PeerSet` invalidates only when the set changes. The peer id is generic
//! (`P: Ord + Clone`); the distributed plane plugs in `PeerId`.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::Context;
use crate::cell::CellHandle;

/// Per-peer liveness state (SWIM).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerState {
    /// Heartbeats current; a valid CRDT sync target.
    Alive,
    /// Phi crossed the threshold; awaiting a refuting heartbeat or the timeout.
    Suspect,
    /// Suspect long enough to declare failed.
    Dead,
    /// Gracefully departed.
    Left,
}

/// A diff event over the membership cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerChangeEvent<P> {
    Joined(P),
    Left(P),
    StateChanged {
        peer: P,
        from: PeerState,
        to: PeerState,
    },
}

/// Tunables for the failure detector + SWIM state machine.
#[derive(Debug, Clone, Copy)]
pub struct MembershipConfig {
    /// `phi > phi_threshold` marks a peer `Suspect`.
    pub phi_threshold: f64,
    /// Ticks a peer stays `Suspect` before being declared `Dead`.
    pub suspect_timeout: u64,
    /// Sliding window size for heartbeat inter-arrival samples.
    pub max_samples: usize,
    /// Floor on the sample standard deviation (avoids div-by-zero).
    pub min_std: f64,
}

impl Default for MembershipConfig {
    fn default() -> Self {
        Self {
            phi_threshold: 8.0,
            suspect_timeout: 5,
            max_samples: 100,
            min_std: 0.1,
        }
    }
}

/// Phi-accrual failure detector over a sliding window of heartbeat
/// inter-arrival times. `phi` is bit-portable across bindings via the Akka-style
/// logistic approximation of the normal CDF.
#[derive(Debug, Clone)]
pub struct PhiAccrual {
    window: VecDeque<f64>,
    max_samples: usize,
    min_std: f64,
    last_heartbeat: Option<u64>,
}

impl PhiAccrual {
    pub fn new(max_samples: usize, min_std: f64) -> Self {
        Self {
            window: VecDeque::new(),
            max_samples: max_samples.max(1),
            min_std,
            last_heartbeat: None,
        }
    }

    /// Record a heartbeat arrival, appending its inter-arrival sample.
    pub fn heartbeat(&mut self, now: u64) {
        if let Some(last) = self.last_heartbeat {
            let interval = now.saturating_sub(last) as f64;
            self.window.push_back(interval);
            while self.window.len() > self.max_samples {
                self.window.pop_front();
            }
        }
        self.last_heartbeat = Some(now);
    }

    fn mean(&self) -> f64 {
        let n = self.window.len() as f64;
        self.window.iter().sum::<f64>() / n
    }

    fn std(&self, mean: f64) -> f64 {
        let n = self.window.len() as f64;
        let var = self
            .window
            .iter()
            .map(|x| (x - mean) * (x - mean))
            .sum::<f64>()
            / n;
        var.sqrt().max(self.min_std)
    }

    /// The suspicion level at `now`. `0.0` when there is no estimate yet.
    pub fn phi(&self, now: u64) -> f64 {
        let Some(last) = self.last_heartbeat else {
            return 0.0;
        };
        if self.window.is_empty() {
            return 0.0;
        }
        let elapsed = now.saturating_sub(last) as f64;
        let mean = self.mean();
        let std = self.std(mean);
        let y = (elapsed - mean) / std;
        let e = (-y * (1.5976 + 0.070566 * y * y)).exp();
        if elapsed > mean {
            -(e / (1.0 + e)).log10()
        } else {
            -(1.0 - 1.0 / (1.0 + e)).log10()
        }
    }
}

struct PeerRecord {
    state: PeerState,
    detector: PhiAccrual,
    suspect_since: Option<u64>,
}

/// The pure membership compute core: the SWIM state machine over a keyed peer
/// map, driven by heartbeats and a logical clock. Emits [`PeerChangeEvent`]s.
pub struct MembershipCore<P> {
    config: MembershipConfig,
    peers: BTreeMap<P, PeerRecord>,
}

impl<P: Ord + Clone> MembershipCore<P> {
    pub fn new(config: MembershipConfig) -> Self {
        Self {
            config,
            peers: BTreeMap::new(),
        }
    }

    fn new_detector(&self) -> PhiAccrual {
        PhiAccrual::new(self.config.max_samples, self.config.min_std)
    }

    /// The current alive peer set (the reactive `PeerSet`).
    pub fn alive_set(&self) -> BTreeSet<P> {
        self.peers
            .iter()
            .filter(|(_, r)| r.state == PeerState::Alive)
            .map(|(p, _)| p.clone())
            .collect()
    }

    /// The state of a known peer.
    pub fn state(&self, peer: &P) -> Option<PeerState> {
        self.peers.get(peer).map(|r| r.state)
    }

    /// Join a peer (or refresh a re-joining one): `Alive` with a fresh detector.
    pub fn join(&mut self, peer: P, now: u64) -> Vec<PeerChangeEvent<P>> {
        let mut detector = self.new_detector();
        detector.heartbeat(now);
        let known = self.peers.contains_key(&peer);
        let prev = self.peers.get(&peer).map(|r| r.state);
        self.peers.insert(
            peer.clone(),
            PeerRecord {
                state: PeerState::Alive,
                detector,
                suspect_since: None,
            },
        );
        match (known, prev) {
            (false, _) => vec![PeerChangeEvent::Joined(peer)],
            (true, Some(PeerState::Alive)) => vec![],
            (true, Some(from)) => vec![PeerChangeEvent::StateChanged {
                peer,
                from,
                to: PeerState::Alive,
            }],
            (true, None) => vec![],
        }
    }

    /// Record a heartbeat. An unknown peer is a join; a `Suspect`/`Dead` peer
    /// returns to `Alive` (SWIM refutation).
    pub fn heartbeat(&mut self, peer: P, now: u64) -> Vec<PeerChangeEvent<P>> {
        let Some(record) = self.peers.get_mut(&peer) else {
            return self.join(peer, now);
        };
        record.detector.heartbeat(now);
        let from = record.state;
        if from != PeerState::Alive && from != PeerState::Left {
            record.state = PeerState::Alive;
            record.suspect_since = None;
            return vec![PeerChangeEvent::StateChanged {
                peer,
                from,
                to: PeerState::Alive,
            }];
        }
        vec![]
    }

    /// Graceful departure.
    pub fn leave(&mut self, peer: P, _now: u64) -> Vec<PeerChangeEvent<P>> {
        let Some(record) = self.peers.get_mut(&peer) else {
            return vec![];
        };
        if record.state == PeerState::Left {
            return vec![];
        }
        record.state = PeerState::Left;
        record.suspect_since = None;
        vec![PeerChangeEvent::Left(peer)]
    }

    /// Advance the clock: escalate `Alive → Suspect` (phi crossed) and
    /// `Suspect → Dead` (timeout elapsed).
    pub fn tick(&mut self, now: u64) -> Vec<PeerChangeEvent<P>> {
        let threshold = self.config.phi_threshold;
        let timeout = self.config.suspect_timeout;
        let mut events = Vec::new();
        for (peer, record) in self.peers.iter_mut() {
            match record.state {
                PeerState::Alive => {
                    if record.detector.phi(now) > threshold {
                        record.state = PeerState::Suspect;
                        record.suspect_since = Some(now);
                        events.push(PeerChangeEvent::StateChanged {
                            peer: peer.clone(),
                            from: PeerState::Alive,
                            to: PeerState::Suspect,
                        });
                    }
                }
                PeerState::Suspect => {
                    let expired = record
                        .suspect_since
                        .is_some_and(|since| now.saturating_sub(since) >= timeout);
                    if expired {
                        record.state = PeerState::Dead;
                        events.push(PeerChangeEvent::StateChanged {
                            peer: peer.clone(),
                            from: PeerState::Suspect,
                            to: PeerState::Dead,
                        });
                    }
                }
                PeerState::Dead | PeerState::Left => {}
            }
        }
        events
    }
}

/// Reactive membership: drives a [`MembershipCore`] and projects the alive set
/// onto a `Cell` so the [`PeerSet`](Self::peer_set) invalidates only on a set
/// change.
pub struct MembershipCell<P> {
    core: RefCell<MembershipCore<P>>,
    peer_set: CellHandle<BTreeSet<P>>,
}

impl<P: Ord + Clone + 'static> MembershipCell<P> {
    pub fn new(ctx: &Context, config: MembershipConfig) -> Self {
        Self {
            core: RefCell::new(MembershipCore::new(config)),
            peer_set: ctx.cell(BTreeSet::new()),
        }
    }

    fn refresh(&self, ctx: &Context) {
        let set = self.core.borrow().alive_set();
        self.peer_set.set(ctx, set);
    }

    pub fn join(&self, ctx: &Context, peer: P, now: u64) -> Vec<PeerChangeEvent<P>> {
        let events = self.core.borrow_mut().join(peer, now);
        self.refresh(ctx);
        events
    }

    pub fn heartbeat(&self, ctx: &Context, peer: P, now: u64) -> Vec<PeerChangeEvent<P>> {
        let events = self.core.borrow_mut().heartbeat(peer, now);
        self.refresh(ctx);
        events
    }

    pub fn leave(&self, ctx: &Context, peer: P, now: u64) -> Vec<PeerChangeEvent<P>> {
        let events = self.core.borrow_mut().leave(peer, now);
        self.refresh(ctx);
        events
    }

    pub fn tick(&self, ctx: &Context, now: u64) -> Vec<PeerChangeEvent<P>> {
        let events = self.core.borrow_mut().tick(now);
        self.refresh(ctx);
        events
    }

    /// The reactive alive peer set (`PeerSet`).
    pub fn peer_set(&self, ctx: &Context) -> BTreeSet<P> {
        self.peer_set.get(ctx)
    }

    /// The backing `PeerSet` cell, for direct subscription.
    pub fn peer_set_cell(&self) -> CellHandle<BTreeSet<P>> {
        self.peer_set
    }

    pub fn state(&self, peer: &P) -> Option<PeerState> {
        self.core.borrow().state(peer)
    }
}

/// The derived reactive alive-peer set — a `Cell<BTreeSet<P>>` handle exposed by
/// [`MembershipCell::peer_set_cell`].
pub type PeerSet<P> = CellHandle<BTreeSet<P>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phi_low_when_current_high_after_gap() {
        let mut d = PhiAccrual::new(100, 0.1);
        d.heartbeat(0);
        d.heartbeat(1);
        d.heartbeat(2);
        d.heartbeat(3);
        assert!(d.phi(3) < 8.0, "phi at last heartbeat should be low");
        assert!(d.phi(100) > 8.0, "phi after a long gap should be high");
    }

    #[test]
    fn lifecycle_transitions() {
        let mut m = MembershipCore::<u64>::new(MembershipConfig::default());
        assert_eq!(m.join(1, 0), vec![PeerChangeEvent::Joined(1)]);
        m.heartbeat(1, 1);
        m.heartbeat(1, 2);
        m.heartbeat(1, 3);
        assert_eq!(m.tick(3), vec![]);
        assert_eq!(m.state(&1), Some(PeerState::Alive));
        assert_eq!(
            m.tick(100),
            vec![PeerChangeEvent::StateChanged {
                peer: 1,
                from: PeerState::Alive,
                to: PeerState::Suspect
            }]
        );
        assert_eq!(
            m.tick(106),
            vec![PeerChangeEvent::StateChanged {
                peer: 1,
                from: PeerState::Suspect,
                to: PeerState::Dead
            }]
        );
        assert!(m.alive_set().is_empty());
    }

    #[test]
    fn heartbeat_refutes_suspicion() {
        let mut m = MembershipCore::<u64>::new(MembershipConfig::default());
        m.join(1, 0);
        m.heartbeat(1, 1);
        m.heartbeat(1, 2);
        m.tick(100); // -> Suspect
        assert_eq!(m.state(&1), Some(PeerState::Suspect));
        let ev = m.heartbeat(1, 101); // refute
        assert_eq!(m.state(&1), Some(PeerState::Alive));
        assert_eq!(
            ev,
            vec![PeerChangeEvent::StateChanged {
                peer: 1,
                from: PeerState::Suspect,
                to: PeerState::Alive
            }]
        );
    }
}
