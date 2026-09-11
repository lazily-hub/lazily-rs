//! Replay-equivalence proof for a reactive graph (`#lzreplayrs`).
//!
//! A durable-execution engine re-runs a workflow from an ordered event log and
//! expects the same decisions. This module makes that expectation **provable**
//! rather than assumed, per `lazily-spec/docs/replay-equivalence.md`:
//!
//! > Given the same event log, a **rebuilt** graph observes the same values at
//! > every checkpoint. Any deviation is a defect in the graph, not a tolerance.
//!
//! The discipline is taken from `tsift`, whose cached excerpts are trustworthy
//! because every one records a body hash and *revalidates it against the source
//! bytes* before the excerpt is returned: a stale body deterministically
//! suppresses the cached answer instead of returning a plausible-looking one.
//! Here the event log is the source bytes.
//!
//! * [`ReplayLog`] is an ordered, strictly-increasing event sequence carrying a
//!   digest over its canonical bytes.
//! * [`ReplayFingerprint`] records, per checkpoint, a digest of every observed
//!   cell value — **and the digest of the log that produced it**, plus the
//!   stride it was sampled at.
//! * [`ReplayHarness::verify`] revalidates that binding *first*. A fingerprint
//!   recorded against a different log fails with
//!   [`ReplayProofError::LogMismatch`] and is never compared, so a stale
//!   fingerprint can neither pass by coincidence (two different logs can settle
//!   to the same final values) nor be misreported as a value divergence.
//! * A value that does differ fails with [`ReplayProofError::Divergence`] naming
//!   the **first** diverging checkpoint and the exact cell label, because a
//!   fingerprint that only covers the final state says the graph is wrong but
//!   not where.
//!
//! # Why there is no hash here
//!
//! The spec deliberately leaves **both** the hash and the byte layout
//! binding-chosen: fingerprints are pinned next to a test in one language and
//! are never exchanged between bindings, so there is nothing to agree on at the
//! byte level. What every binding must agree on is the *equality classes*
//! (mapping/set order irrelevant, sequence order significant, types tagged,
//! members length-framed).
//!
//! lazily-rs keeps every dependency optional and ships no hash crate. Adding
//! blake2 or sha2 for an **optional** (`MAY`) verification facility would buy a
//! mandatory dependency for nothing, so a [`ReplayDigest`] here stores the
//! **exact canonical bytes** of the value. At the sizes a fingerprint covers
//! that is the degenerate strongest choice — zero collision risk rather than
//! merely negligible, and a divergence report can show the bytes that differed
//! instead of two opaque hex strings. The cost is size, which is why
//! [`ReplayDigest`] is compared and stored, never rendered in bulk.
//!
//! # Example
//!
//! ```
//! use lazily::{ReplayEvent, ReplayGraph, ReplayHarness, ReplayLog, ReplayObservation};
//!
//! #[derive(Default)]
//! struct Counter {
//!     total: i64,
//! }
//!
//! impl ReplayGraph for Counter {
//!     fn apply(&mut self, event: &ReplayEvent) {
//!         self.total += event.payload.as_int().unwrap_or(0) as i64;
//!     }
//!
//!     fn observe(&self) -> ReplayObservation {
//!         ReplayObservation::new().with("total", self.total)
//!     }
//! }
//!
//! let log = ReplayLog::from_records([("add", 1), ("add", 2), ("add", 3)]).unwrap();
//! let harness = ReplayHarness::new(Counter::default);
//!
//! let fingerprint = harness.record(&log).unwrap(); // pin it next to the test
//! harness.verify(&log, &fingerprint).unwrap(); // fails if replay diverges
//! harness.prove(&log, 2).unwrap(); // record + re-replay in one call
//! ```

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;

/// The checkpoint sequence number for the state before any event was applied.
pub const INITIAL_SEQ: i64 = -1;

/// How much of a value a divergence report renders inline.
const PREVIEW_LIMIT: usize = 120;

// -- errors -------------------------------------------------------------------

/// A replay-equivalence proof could not be completed as stated.
///
/// The variants are distinct so a driver routes on the **type**, never on a
/// message string: a stale fingerprint, an incomparable stride, a real value
/// divergence and an unencodable observation are four different faults with
/// four different fixes.
#[derive(Debug, Clone, PartialEq)]
pub enum ReplayProofError {
    /// A value has no canonical byte encoding, so it cannot be fingerprinted.
    ///
    /// Reported instead of falling back on the host's default rendering, which
    /// in most languages embeds an address or identity hash and would report a
    /// *false* divergence on every run — the exact failure a replay proof exists
    /// to make impossible, arriving as a flaky test instead of a real one.
    Encoding {
        /// Where in the observed value the undefined member sits.
        path: String,
        /// What the value said it was.
        kind: String,
    },
    /// The fingerprint was recorded against a different event log.
    ///
    /// The tsift rule: revalidate the recorded hash against the source bytes and
    /// deterministically suppress the cached answer when they disagree. A stale
    /// fingerprint is never compared, so it can neither pass by coincidence nor
    /// be misreported as a value divergence.
    LogMismatch {
        /// The log digest the fingerprint was recorded against.
        expected: ReplayDigest,
        /// The digest of the log actually replayed.
        actual: ReplayDigest,
    },
    /// The fingerprint was recorded at a different checkpoint stride.
    ///
    /// A separate fault from [`ReplayProofError::LogMismatch`]: the log is the
    /// right one, but the two checkpoint sequences were never comparable. Equal
    /// log digest **plus** equal stride is what makes them comparable at all.
    StrideMismatch {
        /// The stride the fingerprint was sampled at.
        expected: usize,
        /// The stride this harness samples at.
        actual: usize,
    },
    /// A replayed graph observed a different value than the fingerprint.
    ///
    /// Non-empty by construction, ordered, and truncated to the first diverging
    /// checkpoint — later ones are almost always the same defect carried
    /// forward.
    Divergence(Vec<ReplayDivergence>),
    /// The event log itself is not a well-formed replay source.
    MalformedLog(String),
    /// The fingerprint and the replay agree on log and stride but disagree on
    /// how many checkpoints that produces, so `apply` or `observe` changed.
    CheckpointCount {
        /// Checkpoints the fingerprint carries.
        expected: usize,
        /// Checkpoints the replay produced.
        actual: usize,
    },
    /// A digest could not be parsed back from its hex form.
    MalformedDigest(String),
}

impl ReplayProofError {
    /// The earliest divergence, which is the one worth reading.
    ///
    /// `None` for every other fault — a log mismatch is an unanswerable
    /// question, not a report of zero divergences.
    #[must_use]
    pub fn first_divergence(&self) -> Option<&ReplayDivergence> {
        match self {
            Self::Divergence(divergences) => divergences.first(),
            _ => None,
        }
    }

    /// Every divergence this error carries, empty for every other fault.
    #[must_use]
    pub fn divergences(&self) -> &[ReplayDivergence] {
        match self {
            Self::Divergence(divergences) => divergences,
            _ => &[],
        }
    }
}

impl fmt::Display for ReplayProofError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encoding { path, kind } => write!(
                f,
                "{path}: `{kind}` has no canonical encoding, so it cannot be \
                 fingerprinted; observe a plain value, a record, or a \
                 map/sequence/set of them instead"
            ),
            Self::LogMismatch { expected, actual } => write!(
                f,
                "fingerprint was recorded against a different event log \
                 (fingerprint log digest={}, replayed log digest={}); re-record \
                 the fingerprint against this log",
                expected.short(),
                actual.short()
            ),
            Self::StrideMismatch { expected, actual } => write!(
                f,
                "fingerprint was recorded at stride {expected} but this harness \
                 samples at stride {actual}; re-record it"
            ),
            Self::Divergence(divergences) => {
                let Some(first) = divergences.first() else {
                    return write!(f, "replay diverged from the fingerprint");
                };
                let extra = divergences.len() - 1;
                if extra > 0 {
                    write!(
                        f,
                        "replay diverged from the fingerprint: {first} (+{extra} more)"
                    )
                } else {
                    write!(f, "replay diverged from the fingerprint: {first}")
                }
            }
            Self::MalformedLog(reason) => write!(f, "malformed replay log: {reason}"),
            Self::CheckpointCount { expected, actual } => write!(
                f,
                "fingerprint has {expected} checkpoints but the replay produced \
                 {actual} for the same log and stride"
            ),
            Self::MalformedDigest(reason) => write!(f, "malformed replay digest: {reason}"),
        }
    }
}

impl std::error::Error for ReplayProofError {}

// -- canonical values ---------------------------------------------------------

/// A value the canonical encoding defines.
///
/// Rust moves most of the "undefined value" problem to compile time — a type
/// with no conversion into `ReplayValue` simply cannot be observed. The
/// [`ReplayValue::Opaque`] variant carries the residual *dynamic* case: a value
/// decoded from a foreign source whose shape the encoding does not define. It
/// exists so that case fails loudly at digest time rather than falling back on
/// `Debug`, which embeds addresses for many types.
///
/// `PartialEq` here is structural and is **not** the equality the contract
/// speaks about — `Map` and `Set` compare their member order, which the
/// canonical encoding deliberately erases. Compare [`canonical_digest`] results,
/// not values.
#[derive(Debug, Clone, PartialEq)]
pub enum ReplayValue {
    /// The absent value.
    Null,
    /// A boolean — distinct from the integer `1`.
    Bool(bool),
    /// A signed integer, exact well past 2^53.
    Int(i128),
    /// A float, encoded by its exact IEEE-754 bits.
    Float(f64),
    /// Text — distinct from the equal byte string.
    Str(String),
    /// A byte string — distinct from the equal text.
    Bytes(Vec<u8>),
    /// An ordered sequence; member order is part of the value.
    Seq(Vec<ReplayValue>),
    /// An unordered set; member order is not part of the value.
    Set(Vec<ReplayValue>),
    /// A mapping; entry order is not part of the value.
    Map(Vec<(ReplayValue, ReplayValue)>),
    /// A named record with ordered fields — the shape a struct observes as.
    Record {
        /// The record's type name, part of the value.
        name: String,
        /// Fields in declaration order.
        fields: Vec<(String, ReplayValue)>,
    },
    /// A value the encoding does not define. Digesting one fails with
    /// [`ReplayProofError::Encoding`]; the payload names what it was.
    Opaque(String),
}

impl ReplayValue {
    /// The integer this value carries, if it is one.
    #[must_use]
    pub fn as_int(&self) -> Option<i128> {
        match self {
            Self::Int(value) => Some(*value),
            _ => None,
        }
    }

    /// The text this value carries, if it is text.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(value) => Some(value.as_str()),
            _ => None,
        }
    }

    /// A sequence from anything convertible, for terser call sites.
    pub fn seq<I, V>(items: I) -> Self
    where
        I: IntoIterator<Item = V>,
        V: Into<ReplayValue>,
    {
        Self::Seq(items.into_iter().map(Into::into).collect())
    }

    /// A set from anything convertible. Member order is erased by the encoding.
    pub fn set<I, V>(items: I) -> Self
    where
        I: IntoIterator<Item = V>,
        V: Into<ReplayValue>,
    {
        Self::Set(items.into_iter().map(Into::into).collect())
    }

    /// A mapping from key/value pairs. Entry order is erased by the encoding.
    pub fn map<I, K, V>(entries: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<ReplayValue>,
        V: Into<ReplayValue>,
    {
        Self::Map(
            entries
                .into_iter()
                .map(|(key, value)| (key.into(), value.into()))
                .collect(),
        )
    }

    /// The type name this variant reports in an encoding error.
    fn kind(&self) -> String {
        match self {
            Self::Null => "null".to_owned(),
            Self::Bool(_) => "bool".to_owned(),
            Self::Int(_) => "int".to_owned(),
            Self::Float(_) => "float".to_owned(),
            Self::Str(_) => "str".to_owned(),
            Self::Bytes(_) => "bytes".to_owned(),
            Self::Seq(_) => "seq".to_owned(),
            Self::Set(_) => "set".to_owned(),
            Self::Map(_) => "map".to_owned(),
            Self::Record { name, .. } => format!("record {name}"),
            Self::Opaque(name) => name.clone(),
        }
    }

    fn preview(&self) -> String {
        let text = format!("{self:?}");
        if text.chars().count() > PREVIEW_LIMIT {
            let head: String = text.chars().take(PREVIEW_LIMIT - 1).collect();
            format!("{head}…")
        } else {
            text
        }
    }
}

impl From<bool> for ReplayValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<&str> for ReplayValue {
    fn from(value: &str) -> Self {
        Self::Str(value.to_owned())
    }
}

impl From<String> for ReplayValue {
    fn from(value: String) -> Self {
        Self::Str(value)
    }
}

impl From<f64> for ReplayValue {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl From<f32> for ReplayValue {
    fn from(value: f32) -> Self {
        Self::Float(f64::from(value))
    }
}

macro_rules! replay_value_from_int {
    ($($ty:ty),* $(,)?) => {
        $(
            impl From<$ty> for ReplayValue {
                fn from(value: $ty) -> Self {
                    Self::Int(i128::from(value))
                }
            }
        )*
    };
}

replay_value_from_int!(i8, i16, i32, i64, u8, u16, u32, u64);

impl From<i128> for ReplayValue {
    fn from(value: i128) -> Self {
        Self::Int(value)
    }
}

impl From<usize> for ReplayValue {
    fn from(value: usize) -> Self {
        // usize is at most 64 bits on every target this crate builds for, and
        // i128 is wider than any of them.
        Self::Int(value as i128)
    }
}

impl<T: Into<ReplayValue>> From<Option<T>> for ReplayValue {
    fn from(value: Option<T>) -> Self {
        value.map_or(Self::Null, Into::into)
    }
}

impl<T: Into<ReplayValue>> From<Vec<T>> for ReplayValue {
    fn from(value: Vec<T>) -> Self {
        Self::seq(value)
    }
}

impl<T: Into<ReplayValue> + Clone> From<&[T]> for ReplayValue {
    fn from(value: &[T]) -> Self {
        Self::seq(value.iter().cloned())
    }
}

impl<T: Into<ReplayValue>> From<BTreeSet<T>> for ReplayValue {
    fn from(value: BTreeSet<T>) -> Self {
        Self::set(value)
    }
}

impl<T: Into<ReplayValue>, S> From<HashSet<T, S>> for ReplayValue {
    fn from(value: HashSet<T, S>) -> Self {
        Self::set(value)
    }
}

impl<K: Into<ReplayValue>, V: Into<ReplayValue>> From<BTreeMap<K, V>> for ReplayValue {
    fn from(value: BTreeMap<K, V>) -> Self {
        Self::map(value)
    }
}

impl<K: Into<ReplayValue>, V: Into<ReplayValue>, S> From<HashMap<K, V, S>> for ReplayValue {
    fn from(value: HashMap<K, V, S>) -> Self {
        Self::map(value)
    }
}

// -- canonical encoding -------------------------------------------------------

/// Append `body` under `tag` as `<tag><len>:<body>`.
///
/// The length prefix is the row of the contract that is easiest to get wrong:
/// concatenating member encodings without a length or delimiter makes
/// `["a","bc"]` and `["ab","c"]` identical, and a harness that cannot tell them
/// apart certifies a graph that reshaped its own output.
fn frame(tag: u8, body: &[u8], out: &mut Vec<u8>) {
    out.push(tag);
    out.extend_from_slice(body.len().to_string().as_bytes());
    out.push(b':');
    out.extend_from_slice(body);
}

fn encode(value: &ReplayValue, path: &str, out: &mut Vec<u8>) -> Result<(), ReplayProofError> {
    match value {
        ReplayValue::Null => out.extend_from_slice(b"n0:"),
        ReplayValue::Bool(true) => out.extend_from_slice(b"b1:1"),
        ReplayValue::Bool(false) => out.extend_from_slice(b"b1:0"),
        ReplayValue::Int(value) => frame(b'i', value.to_string().as_bytes(), out),
        // The exact IEEE-754 bits, not a shortest round-trip rendering: the
        // latter folds distinct NaN payloads together, and `-0.0` into `0.0`.
        ReplayValue::Float(value) => {
            frame(b'f', format!("{:016x}", value.to_bits()).as_bytes(), out);
        }
        ReplayValue::Str(value) => frame(b's', value.as_bytes(), out),
        ReplayValue::Bytes(value) => frame(b'y', value, out),
        ReplayValue::Seq(items) => {
            let mut body = Vec::new();
            for (index, item) in items.iter().enumerate() {
                encode(item, &format!("{path}[{index}]"), &mut body)?;
            }
            frame(b'l', &body, out);
        }
        ReplayValue::Set(items) => {
            // Ordered by each member's own encoded bytes: iteration order is not
            // part of the value, and members are not mutually comparable.
            let mut members = Vec::with_capacity(items.len());
            for item in items {
                let mut member = Vec::new();
                encode(item, &format!("{path}{{}}"), &mut member)?;
                members.push(member);
            }
            members.sort_unstable();
            frame(b't', &members.concat(), out);
        }
        ReplayValue::Map(entries) => {
            let mut members = Vec::with_capacity(entries.len());
            for (key, item) in entries {
                let mut member = Vec::new();
                encode(key, &format!("{path}[key]"), &mut member)?;
                encode(item, &format!("{path}[value]"), &mut member)?;
                members.push(member);
            }
            members.sort_unstable();
            frame(b'm', &members.concat(), out);
        }
        ReplayValue::Record { name, fields } => {
            let mut body = Vec::new();
            frame(b's', name.as_bytes(), &mut body);
            for (field, item) in fields {
                frame(b's', field.as_bytes(), &mut body);
                encode(item, &format!("{path}.{field}"), &mut body)?;
            }
            frame(b'd', &body, out);
        }
        ReplayValue::Opaque(_) => {
            return Err(ReplayProofError::Encoding {
                path: path.to_owned(),
                kind: value.kind(),
            });
        }
    }
    Ok(())
}

/// Encode `value` to type-tagged, order-stable, length-framed bytes.
///
/// Mapping and set members are ordered by their own encoded bytes, so insertion
/// and iteration order do not change the result. Every frame is length-prefixed
/// and type-tagged, so `1`, `"1"`, `1.0`, `true` and the byte string `1` encode
/// differently and no concatenation of members can be confused for another.
/// A value the encoding does not define fails with
/// [`ReplayProofError::Encoding`] rather than degrading to a `Debug` rendering.
///
/// # Errors
///
/// [`ReplayProofError::Encoding`] for a [`ReplayValue::Opaque`] anywhere in the
/// value, naming the path that reached it.
pub fn canonical_bytes(value: &ReplayValue) -> Result<Vec<u8>, ReplayProofError> {
    let mut out = Vec::new();
    encode(value, "value", &mut out)?;
    Ok(out)
}

/// The canonical digest of `value`.
///
/// See the module docs: this binding stores the canonical bytes themselves
/// rather than hashing them, because the spec leaves the hash binding-chosen and
/// lazily-rs ships no hash crate for an optional facility.
///
/// # Errors
///
/// As [`canonical_bytes`].
pub fn canonical_digest(value: &ReplayValue) -> Result<ReplayDigest, ReplayProofError> {
    canonical_bytes(value).map(ReplayDigest)
}

/// The canonical digest of a value, in the exact bytes this binding compares.
///
/// Opaque by contract: two fingerprints from different bindings are never
/// compared, so nothing outside this module may depend on the layout.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReplayDigest(Vec<u8>);

impl ReplayDigest {
    /// The digest bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The digest length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the digest is empty, which no digest of a real value is.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// A lowercase hex rendering, for pinning a fingerprint beside a test.
    #[must_use]
    pub fn to_hex(&self) -> String {
        use fmt::Write as _;
        let mut out = String::with_capacity(self.0.len() * 2);
        for byte in &self.0 {
            let _ = write!(out, "{byte:02x}");
        }
        out
    }

    /// Rebuild from [`ReplayDigest::to_hex`].
    ///
    /// # Errors
    ///
    /// [`ReplayProofError::MalformedDigest`] when `hex` is not an even-length
    /// run of hex digits.
    pub fn from_hex(hex: &str) -> Result<Self, ReplayProofError> {
        if !hex.len().is_multiple_of(2) {
            return Err(ReplayProofError::MalformedDigest(format!(
                "odd-length hex string of {} chars",
                hex.len()
            )));
        }
        let bytes = hex.as_bytes();
        let mut out = Vec::with_capacity(bytes.len() / 2);
        for pair in bytes.chunks_exact(2) {
            let text = std::str::from_utf8(pair)
                .map_err(|_| ReplayProofError::MalformedDigest("non-ASCII hex digit".to_owned()))?;
            out.push(u8::from_str_radix(text, 16).map_err(|_| {
                ReplayProofError::MalformedDigest(format!("`{text}` is not a hex byte"))
            })?);
        }
        Ok(Self(out))
    }

    /// A bounded rendering for an error message.
    fn short(&self) -> String {
        let hex = self.to_hex();
        if hex.len() <= 32 {
            hex
        } else {
            format!("{}…({} bytes)", &hex[..32], self.0.len())
        }
    }
}

impl fmt::Debug for ReplayDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ReplayDigest({})", self.short())
    }
}

impl fmt::Display for ReplayDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.short())
    }
}

// -- the log ------------------------------------------------------------------

/// One entry of an ordered event log.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayEvent {
    /// The event's sequence number. Strictly increasing within a log, and
    /// deliberately allowed to be non-contiguous.
    pub seq: u64,
    /// What happened.
    pub name: String,
    /// The event's payload.
    pub payload: ReplayValue,
}

impl ReplayEvent {
    /// A named, numbered event.
    ///
    /// # Errors
    ///
    /// [`ReplayProofError::MalformedLog`] when `name` is empty — an unnamed
    /// event cannot be told from another at the same seq.
    pub fn new(
        seq: u64,
        name: impl Into<String>,
        payload: impl Into<ReplayValue>,
    ) -> Result<Self, ReplayProofError> {
        let name = name.into();
        if name.is_empty() {
            return Err(ReplayProofError::MalformedLog(
                "event name must be non-empty".to_owned(),
            ));
        }
        Ok(Self {
            seq,
            name,
            payload: payload.into(),
        })
    }

    fn as_value(&self) -> ReplayValue {
        ReplayValue::Record {
            name: "ReplayEvent".to_owned(),
            fields: vec![
                ("seq".to_owned(), ReplayValue::Int(i128::from(self.seq))),
                ("name".to_owned(), ReplayValue::Str(self.name.clone())),
                ("payload".to_owned(), self.payload.clone()),
            ],
        }
    }
}

/// An ordered event log with a digest over its canonical bytes.
///
/// Sequence numbers must strictly increase; they do **not** have to be
/// contiguous, because an ack-truncated durable outbox replays real epochs and
/// renumbering them would hide a truncated prefix the log digest otherwise
/// catches.
#[derive(Debug, Clone)]
pub struct ReplayLog {
    events: Vec<ReplayEvent>,
    digest: ReplayDigest,
}

impl ReplayLog {
    /// A log from already-numbered events.
    ///
    /// # Errors
    ///
    /// [`ReplayProofError::MalformedLog`] when the sequence numbers do not
    /// strictly increase, and [`ReplayProofError::Encoding`] when a payload has
    /// no canonical encoding — a log whose digest cannot be taken cannot bind a
    /// fingerprint.
    pub fn new(events: Vec<ReplayEvent>) -> Result<Self, ReplayProofError> {
        let mut previous: Option<u64> = None;
        for event in &events {
            if let Some(previous) = previous
                && event.seq <= previous
            {
                return Err(ReplayProofError::MalformedLog(format!(
                    "event log must be strictly increasing in seq, got {} after {previous}",
                    event.seq
                )));
            }
            previous = Some(event.seq);
        }
        let digest = canonical_digest(&ReplayValue::Seq(
            events.iter().map(ReplayEvent::as_value).collect(),
        ))?;
        Ok(Self { events, digest })
    }

    /// A log from `(name, payload)` pairs, numbered `0..n-1`.
    ///
    /// # Errors
    ///
    /// As [`ReplayLog::new`].
    pub fn from_records<I, N, P>(records: I) -> Result<Self, ReplayProofError>
    where
        I: IntoIterator<Item = (N, P)>,
        N: Into<String>,
        P: Into<ReplayValue>,
    {
        let events = records
            .into_iter()
            .enumerate()
            .map(|(index, (name, payload))| ReplayEvent::new(index as u64, name, payload))
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(events)
    }

    /// The digest over this log's canonical bytes.
    #[must_use]
    pub fn digest(&self) -> &ReplayDigest {
        &self.digest
    }

    /// The events, in order.
    #[must_use]
    pub fn events(&self) -> &[ReplayEvent] {
        &self.events
    }

    /// How many events the log carries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the log is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

impl<'a> IntoIterator for &'a ReplayLog {
    type Item = &'a ReplayEvent;
    type IntoIter = std::slice::Iter<'a, ReplayEvent>;

    fn into_iter(self) -> Self::IntoIter {
        self.events.iter()
    }
}

// -- the observation ----------------------------------------------------------

/// The cell values a fingerprint covers, keyed by a stable label.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReplayObservation {
    cells: BTreeMap<String, ReplayValue>,
}

impl ReplayObservation {
    /// An empty observation.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add `label`, builder-style.
    #[must_use]
    pub fn with(mut self, label: impl Into<String>, value: impl Into<ReplayValue>) -> Self {
        self.insert(label, value);
        self
    }

    /// Add `label`, returning the value it replaced.
    pub fn insert(
        &mut self,
        label: impl Into<String>,
        value: impl Into<ReplayValue>,
    ) -> Option<ReplayValue> {
        self.cells.insert(label.into(), value.into())
    }

    /// The value observed under `label`.
    #[must_use]
    pub fn get(&self, label: &str) -> Option<&ReplayValue> {
        self.cells.get(label)
    }

    /// Every observed label, in sorted order.
    pub fn labels(&self) -> impl Iterator<Item = &str> {
        self.cells.keys().map(String::as_str)
    }

    /// How many cells the observation covers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// Whether the observation covers nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

impl<L: Into<String>, V: Into<ReplayValue>> FromIterator<(L, V)> for ReplayObservation {
    fn from_iter<I: IntoIterator<Item = (L, V)>>(iter: I) -> Self {
        Self {
            cells: iter
                .into_iter()
                .map(|(label, value)| (label.into(), value.into()))
                .collect(),
        }
    }
}

// -- the fingerprint ----------------------------------------------------------

/// Per-cell digests observed after applying events through `seq`.
///
/// `seq` is [`INITIAL_SEQ`] for the state before any event was applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayCheckpoint {
    seq: i64,
    cells: Vec<(String, ReplayDigest)>,
}

impl ReplayCheckpoint {
    /// Digest every observed cell at `seq`.
    ///
    /// # Errors
    ///
    /// [`ReplayProofError::Encoding`] when an observed value has no canonical
    /// encoding.
    pub fn of(seq: i64, observed: &ReplayObservation) -> Result<Self, ReplayProofError> {
        let mut cells = Vec::with_capacity(observed.cells.len());
        for (label, value) in &observed.cells {
            cells.push((label.clone(), canonical_digest(value)?));
        }
        // `observed` is a BTreeMap, so this is already label-sorted; make the
        // invariant explicit rather than inherited.
        cells.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(Self { seq, cells })
    }

    /// The event sequence number this checkpoint covers.
    #[must_use]
    pub fn seq(&self) -> i64 {
        self.seq
    }

    /// The label → digest pairs, sorted by label.
    #[must_use]
    pub fn cells(&self) -> &[(String, ReplayDigest)] {
        &self.cells
    }

    /// The digest recorded for `label`.
    #[must_use]
    pub fn get(&self, label: &str) -> Option<&ReplayDigest> {
        self.cells
            .iter()
            .find(|(candidate, _)| candidate == label)
            .map(|(_, digest)| digest)
    }

    fn as_value(&self) -> ReplayValue {
        ReplayValue::Record {
            name: "ReplayCheckpoint".to_owned(),
            fields: vec![
                ("seq".to_owned(), ReplayValue::Int(i128::from(self.seq))),
                (
                    "cells".to_owned(),
                    ReplayValue::Seq(
                        self.cells
                            .iter()
                            .map(|(label, digest)| {
                                ReplayValue::Seq(vec![
                                    ReplayValue::Str(label.clone()),
                                    ReplayValue::Bytes(digest.0.clone()),
                                ])
                            })
                            .collect(),
                    ),
                ),
            ],
        }
    }
}

/// A recorded, log-bound observation of a replayed graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayFingerprint {
    log_digest: ReplayDigest,
    stride: usize,
    checkpoints: Vec<ReplayCheckpoint>,
    digest: ReplayDigest,
}

impl ReplayFingerprint {
    /// Assemble a fingerprint from its parts.
    ///
    /// # Errors
    ///
    /// [`ReplayProofError::MalformedLog`] when `stride` is zero or the
    /// checkpoint list is empty — a fingerprint needs at least the initial
    /// checkpoint.
    pub fn new(
        log_digest: ReplayDigest,
        stride: usize,
        checkpoints: Vec<ReplayCheckpoint>,
    ) -> Result<Self, ReplayProofError> {
        if stride < 1 {
            return Err(ReplayProofError::MalformedLog(
                "stride must be >= 1".to_owned(),
            ));
        }
        if checkpoints.is_empty() {
            return Err(ReplayProofError::MalformedLog(
                "a fingerprint needs at least the initial checkpoint".to_owned(),
            ));
        }
        let digest = canonical_digest(&ReplayValue::Record {
            name: "ReplayFingerprint".to_owned(),
            fields: vec![
                (
                    "log_digest".to_owned(),
                    ReplayValue::Bytes(log_digest.0.clone()),
                ),
                ("stride".to_owned(), ReplayValue::Int(stride as i128)),
                (
                    "checkpoints".to_owned(),
                    ReplayValue::Seq(checkpoints.iter().map(ReplayCheckpoint::as_value).collect()),
                ),
            ],
        })?;
        Ok(Self {
            log_digest,
            stride,
            checkpoints,
            digest,
        })
    }

    /// The digest of the log this fingerprint was recorded against.
    #[must_use]
    pub fn log_digest(&self) -> &ReplayDigest {
        &self.log_digest
    }

    /// The checkpoint stride this fingerprint was sampled at.
    #[must_use]
    pub fn stride(&self) -> usize {
        self.stride
    }

    /// Every checkpoint, in order.
    #[must_use]
    pub fn checkpoints(&self) -> &[ReplayCheckpoint] {
        &self.checkpoints
    }

    /// The last checkpoint — the end state of the replay.
    ///
    /// # Panics
    ///
    /// Never: a fingerprint carries at least the initial checkpoint by
    /// construction.
    #[must_use]
    pub fn final_checkpoint(&self) -> &ReplayCheckpoint {
        self.checkpoints
            .last()
            .expect("a fingerprint carries at least the initial checkpoint")
    }

    /// A digest over the whole fingerprint, for pinning it as one value.
    #[must_use]
    pub fn digest(&self) -> &ReplayDigest {
        &self.digest
    }
}

/// Why one cell did not replay to its recorded digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DivergenceKind {
    /// Both sides carry the label and the digests differ.
    Value,
    /// The fingerprint carries the label and the replay did not observe it.
    Missing,
    /// The replay observed a label the fingerprint does not carry.
    Unexpected,
}

impl DivergenceKind {
    /// The cross-binding spelling the corpus uses.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Value => "value",
            Self::Missing => "missing",
            Self::Unexpected => "unexpected",
        }
    }
}

impl fmt::Display for DivergenceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One cell that did not replay to its recorded digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayDivergence {
    /// The checkpoint's sequence number, [`INITIAL_SEQ`] for the initial state.
    pub seq: i64,
    /// The cell label that differed.
    pub label: String,
    /// Which of the three ways it differed.
    pub kind: DivergenceKind,
    /// The digest the fingerprint carries, absent for an unexpected label.
    pub expected: Option<ReplayDigest>,
    /// The digest the replay produced, absent for a missing label.
    pub actual: Option<ReplayDigest>,
    /// A bounded rendering of the observed value, when one was observed.
    pub preview: Option<String>,
}

impl fmt::Display for ReplayDivergence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let where_ = if self.seq == INITIAL_SEQ {
            "initial state".to_owned()
        } else {
            format!("event seq={}", self.seq)
        };
        match self.kind {
            DivergenceKind::Missing => write!(
                f,
                "{where_}: cell `{}` was not observed on replay",
                self.label
            ),
            DivergenceKind::Unexpected => write!(
                f,
                "{where_}: cell `{}` appeared on replay but is not in the fingerprint",
                self.label
            ),
            DivergenceKind::Value => {
                let expected = self
                    .expected
                    .as_ref()
                    .map_or_else(String::new, ReplayDigest::short);
                let actual = self
                    .actual
                    .as_ref()
                    .map_or_else(String::new, ReplayDigest::short);
                let preview = self
                    .preview
                    .as_ref()
                    .map_or_else(String::new, |p| format!(", observed {p}"));
                write!(
                    f,
                    "{where_}: cell `{}` expected {expected} but replayed {actual}{preview}",
                    self.label
                )
            }
        }
    }
}

// -- the graph under proof ----------------------------------------------------

/// What the harness needs from the graph it rebuilds.
///
/// `apply` advances the graph by exactly one event; `observe` returns the cell
/// values the fingerprint covers, keyed by a stable label.
pub trait ReplayGraph {
    /// Advance by exactly one event.
    fn apply(&mut self, event: &ReplayEvent);

    /// The cell values this fingerprint covers.
    fn observe(&self) -> ReplayObservation;
}

/// Rebuild a graph from an event log and prove it replays identically.
///
/// The builder is called once per replay and must return a **fresh** graph — a
/// harness that reuses one instance proves nothing, since the state it would
/// compare against is the state it already has. That is why the harness takes a
/// builder rather than a graph.
///
/// `stride` checkpoints every `stride`-th event; the initial state and the final
/// state are always checkpointed. It is recorded in the fingerprint, so a
/// fingerprint cannot be compared against a replay that sampled differently.
#[derive(Debug, Clone)]
pub struct ReplayHarness<B> {
    build: B,
    stride: usize,
}

impl<B, G> ReplayHarness<B>
where
    B: Fn() -> G,
    G: ReplayGraph,
{
    /// A harness checkpointing every event (`stride = 1`), the default the
    /// contract requires for divergence localization.
    pub fn new(build: B) -> Self {
        Self { build, stride: 1 }
    }

    /// A harness sampling every `stride`-th event, for a long log.
    ///
    /// # Errors
    ///
    /// [`ReplayProofError::MalformedLog`] when `stride` is zero.
    pub fn with_stride(build: B, stride: usize) -> Result<Self, ReplayProofError> {
        if stride < 1 {
            return Err(ReplayProofError::MalformedLog(
                "stride must be >= 1".to_owned(),
            ));
        }
        Ok(Self { build, stride })
    }

    /// The checkpoint stride this harness samples at.
    pub fn stride(&self) -> usize {
        self.stride
    }

    /// Replay `log` once and record what the graph observed.
    ///
    /// # Errors
    ///
    /// [`ReplayProofError::Encoding`] when an observed value has no canonical
    /// encoding.
    pub fn record(&self, log: &ReplayLog) -> Result<ReplayFingerprint, ReplayProofError> {
        Ok(self.replay(log)?.0)
    }

    /// Replay `log` and return the divergences from `fingerprint`.
    ///
    /// The reporting form: a value divergence is returned rather than raised, so
    /// a caller can report all of them.
    ///
    /// # Errors
    ///
    /// Still fails with [`ReplayProofError::LogMismatch`] for a fingerprint
    /// recorded against a different log, and [`ReplayProofError::StrideMismatch`]
    /// for one recorded at a different stride: a stale fingerprint is an
    /// unanswerable question, not a report.
    pub fn check(
        &self,
        log: &ReplayLog,
        fingerprint: &ReplayFingerprint,
    ) -> Result<Vec<ReplayDivergence>, ReplayProofError> {
        let (replayed, observed) = self.replay(log)?;
        revalidate(fingerprint, &replayed)?;
        compare(fingerprint, &replayed, &observed)
    }

    /// Replay `log` and fail unless it matches `fingerprint` exactly.
    ///
    /// Returns the freshly recorded fingerprint, which equals `fingerprint`.
    ///
    /// # Errors
    ///
    /// [`ReplayProofError::LogMismatch`] or [`ReplayProofError::StrideMismatch`]
    /// before any value is compared, then [`ReplayProofError::Divergence`]
    /// naming the first diverging checkpoint.
    pub fn verify(
        &self,
        log: &ReplayLog,
        fingerprint: &ReplayFingerprint,
    ) -> Result<ReplayFingerprint, ReplayProofError> {
        let (replayed, observed) = self.replay(log)?;
        revalidate(fingerprint, &replayed)?;
        let divergences = compare(fingerprint, &replayed, &observed)?;
        if !divergences.is_empty() {
            return Err(ReplayProofError::Divergence(divergences));
        }
        Ok(replayed)
    }

    /// Record `log` and re-replay it, failing on any divergence.
    ///
    /// The self-check: no external fingerprint is needed to catch a graph that
    /// is not a pure function of its log, because two replays of the same log in
    /// the same process already disagree.
    ///
    /// # Errors
    ///
    /// As [`ReplayHarness::verify`], plus [`ReplayProofError::MalformedLog`]
    /// when `replays` is below two — one replay compares against nothing.
    pub fn prove(
        &self,
        log: &ReplayLog,
        replays: usize,
    ) -> Result<ReplayFingerprint, ReplayProofError> {
        if replays < 2 {
            return Err(ReplayProofError::MalformedLog(format!(
                "prove needs at least 2 replays to compare, got {replays}"
            )));
        }
        let fingerprint = self.record(log)?;
        for _ in 1..replays {
            self.verify(log, &fingerprint)?;
        }
        Ok(fingerprint)
    }

    fn replay(
        &self,
        log: &ReplayLog,
    ) -> Result<(ReplayFingerprint, Vec<ReplayObservation>), ReplayProofError> {
        let mut graph = (self.build)();
        let mut sample = graph.observe();
        let mut checkpoints = vec![ReplayCheckpoint::of(INITIAL_SEQ, &sample)?];
        let mut observed = vec![sample];
        let total = log.len();
        for (index, event) in log.events().iter().enumerate() {
            graph.apply(event);
            if (index + 1).is_multiple_of(self.stride) || index + 1 == total {
                sample = graph.observe();
                checkpoints.push(ReplayCheckpoint::of(
                    i64::try_from(event.seq).unwrap_or(i64::MAX),
                    &sample,
                )?);
                observed.push(sample);
            }
        }
        let fingerprint = ReplayFingerprint::new(log.digest().clone(), self.stride, checkpoints)?;
        Ok((fingerprint, observed))
    }
}

/// Bind the fingerprint to these exact log bytes before comparing any value.
fn revalidate(
    fingerprint: &ReplayFingerprint,
    replayed: &ReplayFingerprint,
) -> Result<(), ReplayProofError> {
    if fingerprint.log_digest != replayed.log_digest {
        return Err(ReplayProofError::LogMismatch {
            expected: fingerprint.log_digest.clone(),
            actual: replayed.log_digest.clone(),
        });
    }
    if fingerprint.stride != replayed.stride {
        return Err(ReplayProofError::StrideMismatch {
            expected: fingerprint.stride,
            actual: replayed.stride,
        });
    }
    Ok(())
}

fn compare(
    expected: &ReplayFingerprint,
    actual: &ReplayFingerprint,
    observed: &[ReplayObservation],
) -> Result<Vec<ReplayDivergence>, ReplayProofError> {
    let mut divergences: Vec<ReplayDivergence> = Vec::new();
    for (index, (want, got)) in expected
        .checkpoints
        .iter()
        .zip(actual.checkpoints.iter())
        .enumerate()
    {
        let mut labels: BTreeSet<&str> = BTreeSet::new();
        labels.extend(want.cells.iter().map(|(label, _)| label.as_str()));
        labels.extend(got.cells.iter().map(|(label, _)| label.as_str()));
        for label in labels {
            let want_digest = want.get(label);
            let got_digest = got.get(label);
            if want_digest == got_digest {
                continue;
            }
            let kind = match (want_digest, got_digest) {
                (Some(_), None) => DivergenceKind::Missing,
                (None, Some(_)) => DivergenceKind::Unexpected,
                _ => DivergenceKind::Value,
            };
            let preview = observed
                .get(index)
                .and_then(|sample| sample.get(label))
                .map(ReplayValue::preview);
            divergences.push(ReplayDivergence {
                seq: want.seq,
                label: label.to_owned(),
                kind,
                expected: want_digest.cloned(),
                actual: got_digest.cloned(),
                preview,
            });
        }
        if !divergences.is_empty() {
            // The first diverging checkpoint is the actionable one; later ones
            // are almost always the same defect carried forward.
            break;
        }
    }
    if expected.checkpoints.len() != actual.checkpoints.len() && divergences.is_empty() {
        // Same log digest and stride, so this cannot come from sampling — it
        // means `observe` or `apply` changed the checkpoint count.
        return Err(ReplayProofError::CheckpointCount {
            expected: expected.checkpoints.len(),
            actual: actual.checkpoints.len(),
        });
    }
    Ok(divergences)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The corpus's canonical `accumulator` / `drifting_accumulator` subject.
    struct Accumulator {
        sum: i128,
        names: Vec<String>,
        drift_at: Option<u64>,
        drift: i128,
    }

    impl Accumulator {
        fn plain() -> Self {
            Self {
                sum: 0,
                names: Vec::new(),
                drift_at: None,
                drift: 0,
            }
        }

        fn drifting(drift_at: u64, drift: i128) -> Self {
            Self {
                sum: 0,
                names: Vec::new(),
                drift_at: Some(drift_at),
                drift,
            }
        }
    }

    impl ReplayGraph for Accumulator {
        fn apply(&mut self, event: &ReplayEvent) {
            self.sum += event.payload.as_int().unwrap_or(0);
            self.names.push(event.name.clone());
            if self.drift_at == Some(event.seq) {
                self.sum += self.drift;
            }
        }

        fn observe(&self) -> ReplayObservation {
            ReplayObservation::new()
                .with("sum", self.sum)
                .with("names", ReplayValue::seq(self.names.iter().cloned()))
        }
    }

    fn log_abc() -> ReplayLog {
        ReplayLog::from_records([("add", 1), ("add", 2), ("add", 3)]).unwrap()
    }

    #[test]
    fn frames_are_length_prefixed_so_a_concatenation_is_unambiguous() {
        let a_bc = ReplayValue::seq(["a", "bc"]);
        let ab_c = ReplayValue::seq(["ab", "c"]);
        assert_ne!(canonical_digest(&a_bc), canonical_digest(&ab_c));
    }

    #[test]
    fn mapping_and_set_order_are_not_part_of_the_value() {
        assert_eq!(
            canonical_digest(&ReplayValue::map([("a", 1), ("b", 2)])),
            canonical_digest(&ReplayValue::map([("b", 2), ("a", 1)])),
        );
        assert_eq!(
            canonical_digest(&ReplayValue::set([1, 2, 3])),
            canonical_digest(&ReplayValue::set([3, 1, 2])),
        );
    }

    #[test]
    fn sequence_order_is_part_of_the_value() {
        assert_ne!(
            canonical_digest(&ReplayValue::seq([1, 2])),
            canonical_digest(&ReplayValue::seq([2, 1])),
        );
    }

    #[test]
    fn types_are_tagged() {
        let one = canonical_digest(&ReplayValue::Int(1)).unwrap();
        for other in [
            ReplayValue::Str("1".to_owned()),
            ReplayValue::Float(1.0),
            ReplayValue::Bool(true),
            ReplayValue::Bytes(vec![b'1']),
        ] {
            assert_ne!(one, canonical_digest(&other).unwrap(), "{other:?}");
        }
    }

    #[test]
    fn integers_past_two_to_the_fifty_three_stay_distinct() {
        assert_ne!(
            canonical_digest(&ReplayValue::Int(9_007_199_254_740_993)),
            canonical_digest(&ReplayValue::Int(9_007_199_254_740_994)),
        );
    }

    #[test]
    fn an_undefined_value_fails_loudly() {
        let error = canonical_digest(&ReplayValue::Opaque("MyThing".to_owned())).unwrap_err();
        assert!(
            matches!(&error, ReplayProofError::Encoding { kind, .. } if kind == "MyThing"),
            "{error:?}"
        );
    }

    #[test]
    fn an_undefined_member_names_its_path() {
        let value = ReplayValue::Record {
            name: "Wrapper".to_owned(),
            fields: vec![("inner".to_owned(), ReplayValue::Opaque("Thing".to_owned()))],
        };
        let error = canonical_digest(&value).unwrap_err();
        assert!(
            matches!(&error, ReplayProofError::Encoding { path, .. } if path == "value.inner"),
            "{error:?}"
        );
    }

    #[test]
    fn float_bits_distinguish_signed_zero_and_nan_payloads() {
        assert_ne!(
            canonical_digest(&ReplayValue::Float(0.0)),
            canonical_digest(&ReplayValue::Float(-0.0)),
        );
        assert_eq!(
            canonical_digest(&ReplayValue::Float(f64::NAN)),
            canonical_digest(&ReplayValue::Float(f64::NAN)),
        );
    }

    #[test]
    fn a_log_must_strictly_increase() {
        let events = vec![
            ReplayEvent::new(1, "add", 1).unwrap(),
            ReplayEvent::new(1, "add", 2).unwrap(),
        ];
        assert!(matches!(
            ReplayLog::new(events),
            Err(ReplayProofError::MalformedLog(_))
        ));
    }

    #[test]
    fn a_log_may_be_non_contiguous() {
        let events = vec![
            ReplayEvent::new(7, "add", 1).unwrap(),
            ReplayEvent::new(19, "add", 2).unwrap(),
        ];
        let log = ReplayLog::new(events).unwrap();
        let fingerprint = ReplayHarness::new(Accumulator::plain).record(&log).unwrap();
        let seqs: Vec<i64> = fingerprint
            .checkpoints()
            .iter()
            .map(ReplayCheckpoint::seq)
            .collect();
        assert_eq!(seqs, vec![INITIAL_SEQ, 7, 19]);
    }

    #[test]
    fn two_logs_with_the_same_final_value_have_different_digests() {
        let a = log_abc();
        let b = ReplayLog::from_records([("add", 3), ("add", 2), ("add", 1)]).unwrap();
        assert_ne!(a.digest(), b.digest());

        let harness = ReplayHarness::new(Accumulator::plain);
        let fp_a = harness.record(&a).unwrap();
        // The value comparison WOULD pass — both sum to 6 — which is exactly why
        // the log binding is revalidated first.
        assert_eq!(
            fp_a.final_checkpoint().get("sum"),
            harness.record(&b).unwrap().final_checkpoint().get("sum"),
        );
        assert!(matches!(
            harness.verify(&b, &fp_a),
            Err(ReplayProofError::LogMismatch { .. })
        ));
        assert!(matches!(
            harness.check(&b, &fp_a),
            Err(ReplayProofError::LogMismatch { .. })
        ));
    }

    #[test]
    fn divergence_is_localized_to_the_first_diverging_checkpoint() {
        let log =
            ReplayLog::from_records([("add", 1), ("add", 2), ("add", 3), ("add", 4)]).unwrap();
        let clean = ReplayHarness::new(Accumulator::plain);
        let fingerprint = clean.record(&log).unwrap();

        let drifting = ReplayHarness::new(|| Accumulator::drifting(1, 100));
        let error = drifting.verify(&log, &fingerprint).unwrap_err();
        let first = error.first_divergence().expect("a value divergence");
        assert_eq!(first.seq, 1);
        assert_eq!(first.label, "sum");
        assert_eq!(first.kind, DivergenceKind::Value);
        // seq 0 matched, and only the first diverging checkpoint is reported.
        assert!(error.divergences().iter().all(|d| d.seq == 1));
    }

    #[test]
    fn a_stride_mismatch_is_its_own_fault() {
        let log =
            ReplayLog::from_records([("add", 1), ("add", 2), ("add", 3), ("add", 4)]).unwrap();
        let sparse = ReplayHarness::with_stride(Accumulator::plain, 2).unwrap();
        let fp_sparse = sparse.record(&log).unwrap();
        let seqs: Vec<i64> = fp_sparse
            .checkpoints()
            .iter()
            .map(ReplayCheckpoint::seq)
            .collect();
        assert_eq!(seqs, vec![INITIAL_SEQ, 1, 3]);

        let dense = ReplayHarness::new(Accumulator::plain);
        assert!(matches!(
            dense.verify(&log, &fp_sparse),
            Err(ReplayProofError::StrideMismatch {
                expected: 2,
                actual: 1
            })
        ));
        sparse.verify(&log, &fp_sparse).unwrap();
    }

    #[test]
    fn a_missing_label_is_not_a_value_divergence() {
        struct Shrinking(bool);
        impl ReplayGraph for Shrinking {
            fn apply(&mut self, _event: &ReplayEvent) {
                self.0 = false;
            }
            fn observe(&self) -> ReplayObservation {
                let mut out = ReplayObservation::new().with("always", 1);
                if self.0 {
                    out.insert("sometimes", 1);
                }
                out
            }
        }
        let log = log_abc();
        let harness = ReplayHarness::new(|| Shrinking(true));
        let fingerprint = harness.record(&log).unwrap();
        let gone = ReplayHarness::new(|| Shrinking(false));
        let error = gone.verify(&log, &fingerprint).unwrap_err();
        let first = error.first_divergence().unwrap();
        assert_eq!(first.kind, DivergenceKind::Missing);
        assert_eq!(first.label, "sometimes");
        assert_eq!(first.seq, INITIAL_SEQ);
    }

    #[test]
    fn prove_catches_a_graph_that_is_not_a_function_of_its_log() {
        use std::cell::Cell as StdCell;
        thread_local! {
            static BUILDS: StdCell<i128> = const { StdCell::new(0) };
        }
        struct Impure(i128);
        impl ReplayGraph for Impure {
            fn apply(&mut self, event: &ReplayEvent) {
                self.0 += event.payload.as_int().unwrap_or(0);
            }
            fn observe(&self) -> ReplayObservation {
                ReplayObservation::new().with("sum", self.0)
            }
        }
        let harness = ReplayHarness::new(|| {
            Impure(BUILDS.with(|b| {
                b.set(b.get() + 1);
                b.get()
            }))
        });
        assert!(matches!(
            harness.prove(&log_abc(), 2),
            Err(ReplayProofError::Divergence(_))
        ));
    }

    #[test]
    fn prove_needs_two_replays_to_compare() {
        let harness = ReplayHarness::new(Accumulator::plain);
        assert!(matches!(
            harness.prove(&log_abc(), 1),
            Err(ReplayProofError::MalformedLog(_))
        ));
    }

    #[test]
    fn an_unencodable_observation_fails_loudly() {
        struct Opaque;
        impl ReplayGraph for Opaque {
            fn apply(&mut self, _event: &ReplayEvent) {}
            fn observe(&self) -> ReplayObservation {
                ReplayObservation::new().with("thing", ReplayValue::Opaque("Thing".to_owned()))
            }
        }
        assert!(matches!(
            ReplayHarness::new(|| Opaque).record(&log_abc()),
            Err(ReplayProofError::Encoding { .. })
        ));
    }

    #[test]
    fn a_digest_round_trips_through_hex() {
        let digest = canonical_digest(&ReplayValue::seq(["a", "bc"])).unwrap();
        assert_eq!(ReplayDigest::from_hex(&digest.to_hex()).unwrap(), digest);
        assert!(matches!(
            ReplayDigest::from_hex("abc"),
            Err(ReplayProofError::MalformedDigest(_))
        ));
        assert!(matches!(
            ReplayDigest::from_hex("zz"),
            Err(ReplayProofError::MalformedDigest(_))
        ));
    }

    #[test]
    fn an_empty_event_name_is_refused() {
        assert!(matches!(
            ReplayEvent::new(0, "", 1),
            Err(ReplayProofError::MalformedLog(_))
        ));
    }

    #[test]
    fn a_fingerprint_digest_covers_the_log_binding_and_the_stride() {
        let log = log_abc();
        let one = ReplayHarness::new(Accumulator::plain).record(&log).unwrap();
        let two = ReplayHarness::with_stride(Accumulator::plain, 2)
            .unwrap()
            .record(&log)
            .unwrap();
        assert_ne!(one.digest(), two.digest());
    }
}
