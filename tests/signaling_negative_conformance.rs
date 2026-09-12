#![cfg(feature = "signaling-client")]

mod common;

use common::Expect;
use lazily::{ClientMessage, ServerMessage};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const FRAMES_PATH: common::SpecDir = common::SpecDir("signaling/frames.json");
const SESSION_PATH: common::SpecDir = common::SpecDir("signaling/anti_spoof_session.json");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FrameCase {
    label: String,
    direction: String,
    wire: Value,
    /// The frame's message variant. Carried by every positive frame and read by
    /// NOTHING until `#lznullformblind` — serde drops an undeclared field
    /// silently, so an unread label on a plain struct is invisible to every
    /// rung: the assertion-key guard only covers blocks a runner bound, and this
    /// one was never in a block at all. It is held to the DECODED frame below,
    /// not to the input wire, so a label naming one variant over a frame the
    /// codec reads as another reddens. `Option` because the `rejects` cases
    /// share this struct and carry no variant.
    #[serde(default)]
    variant: Option<String>,
    /// Language-agnostic claims about the DECODED frame. Read by nothing until
    /// `#lzassertunknownkeys`: the runner round-tripped the wire and never
    /// checked a single one, so `server_stamped_from` / `roster_excludes_self`
    /// were carried by the corpus and asserted by no binding here.
    #[serde(default)]
    assertions: Value,
    #[serde(default)]
    #[allow(dead_code)]
    #[serde(rename = "reason")]
    frame_reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FramesFixture {
    #[allow(dead_code)]
    #[serde(rename = "description")]
    frames_description: String,
    protocol_version: u64,
    kind: String,
    frames: Vec<FrameCase>,
    rejects: Vec<FrameCase>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionInput {
    /// Which connection sent the frame. The server's whole anti-spoof job is to
    /// stamp `from` off THIS, never off anything the client wrote.
    #[serde(default)]
    conn: Option<String>,
    recv: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionReject {
    label: String,
    input: SessionInput,
    #[allow(dead_code)]
    #[serde(rename = "reason")]
    session_reason: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionStep {
    input: SessionInput,
    /// The frames this step expects the server to emit, one element per
    /// emission. Kept as raw `Value`s rather than a typed `SessionEmit` struct
    /// (`#lzarrayelementsites`): each element is an assertion block in its own
    /// right, and it is the tracker — not a `deny_unknown_fields` attribute —
    /// that has to own the key obligation. The attribute refused an unknown key
    /// at PARSE time and was invisible to every rung above, so the two keys it
    /// did carry (`to`, `frame`) reached no comparison: `to` was bound to a
    /// field named `emit_to` and read by nothing, and `frame` was consulted
    /// per-key for `type` / `peer` / `peers` / `from` and never as a whole.
    #[serde(default)]
    expect: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionFixture {
    #[allow(dead_code)]
    #[serde(rename = "description")]
    session_description: String,
    protocol_version: u64,
    kind: String,
    mode: String,
    #[serde(default)]
    steps: Vec<SessionStep>,
    rejects: Vec<SessionReject>,
    /// Session-level claims, asserted over the transcript's own emitted frames
    /// decoded through the shipped codec (`#lznullformblind`).
    #[serde(default)]
    assertions: Value,
}

fn decode(direction: &str, wire: &Value) -> Result<Value, String> {
    let bytes = serde_json::to_vec(wire).expect("fixture wire serializes");
    match direction {
        "client" => serde_json::from_slice::<ClientMessage>(&bytes)
            .and_then(serde_json::to_value)
            .map_err(|error| error.to_string()),
        "server" => ServerMessage::from_json_slice(&bytes)
            .and_then(serde_json::to_value)
            .map_err(|error| error.to_string()),
        // Fail-closed (`#lzscenariobodyskip`). This used to return `Err`, which
        // the negative loop below asserts with `is_err()` — so a `rejects` case
        // carrying a misspelled `direction` was "rejected" because the RUNNER
        // did not recognise the direction, never because the codec rejected the
        // frame. The fixture's whole claim went unexercised and the case passed.
        other => panic!("unknown fixture direction {other:?}"),
    }
}

#[test]
fn signaling_frames_replay_positive_and_negative_cases() {
    let raw =
        common::spec_read_to_string(FRAMES_PATH.path()).expect("read signaling frames fixture");
    let fixture: FramesFixture =
        serde_json::from_str(&raw).expect("parse signaling frames fixture");
    assert_eq!(fixture.protocol_version, 1);
    assert_eq!(fixture.kind, "SignalingFrames");

    let mut variants_replayed = 0usize;
    for case in fixture.frames {
        let actual = decode(&case.direction, &case.wire)
            .unwrap_or_else(|error| panic!("{} should decode: {error}", case.label));
        assert_eq!(
            actual, case.wire,
            "{} should round-trip exactly",
            case.label
        );
        // The label held to the DECODE (`#lznullformblind`). Every positive
        // frame declares its variant and nothing checked it; the discriminator
        // the codec really produced is the round-tripped frame's `type`.
        let variant = case
            .variant
            .as_deref()
            .unwrap_or_else(|| panic!("{}: a positive frame must declare a variant", case.label));
        assert_eq!(
            actual["type"].as_str(),
            Some(variant),
            "{}: the frame's `variant` label and the decoded discriminator disagree",
            case.label
        );
        variants_replayed += 1;
        assert_frame_assertions(&case, &actual);
    }
    assert_eq!(
        variants_replayed, 17,
        "every positive frame must reach the variant check"
    );

    for case in fixture.rejects {
        assert!(
            decode(&case.direction, &case.wire).is_err(),
            "{} should be rejected",
            case.label
        );
    }
}

/// Assert every key of a frame's `assertions` block against the DECODED frame.
///
/// Reading them off `case.wire` would assert the fixture against itself; these
/// read `actual`, the value the codec produced.
fn assert_frame_assertions(case: &FrameCase, actual: &Value) {
    let exp = Expect::new(
        FRAMES_PATH,
        format!("frames[{}].assertions", case.label),
        &case.assertions,
    );
    // Each key is optional per frame, so every comparison is bound to the key's
    // *presence*: a bare read on a frame that does not carry the key marked it
    // consumed while comparing nothing (`#lzconsumednotasserted`).
    exp.assert_key_if_present("peer", |want| {
        assert_eq!(
            actual["peer"].as_u64(),
            want.as_u64(),
            "{}: peer",
            case.label
        );
    });
    exp.assert_key_if_present("to", |want| {
        assert_eq!(actual["to"].as_u64(), want.as_u64(), "{}: to", case.label);
    });
    exp.assert_key_if_present("from", |want| {
        assert_eq!(
            actual["from"].as_u64(),
            want.as_u64(),
            "{}: from",
            case.label
        );
    });
    exp.assert_key_if_present("code", |want| {
        assert_eq!(
            actual["code"].as_str(),
            want.as_str(),
            "{}: code",
            case.label
        );
    });
    exp.assert_key_if_present("has_capabilities", |want| {
        assert_eq!(
            actual.get("capabilities").is_some_and(|c| !c.is_null()),
            want.as_bool().expect("has_capabilities"),
            "{}: has_capabilities",
            case.label
        );
    });
    exp.assert_key_if_present("capabilities", |want| {
        assert_eq!(
            actual["capabilities"].as_array(),
            want.as_array(),
            "{}: capabilities",
            case.label
        );
    });
    exp.assert_key_if_present("peers", |want| {
        assert_eq!(
            actual["peers"].as_array(),
            want.as_array(),
            "{}: peers",
            case.label
        );
    });
    exp.assert_key_if_present("roster_excludes_self", |want| {
        // A welcome roster that lists the joining peer is the spoof this key
        // pins; `rejects` covers the wire-level form, this covers the decoded one.
        let self_peer = actual["peer"].as_u64();
        let excluded = actual["peers"]
            .as_array()
            .expect("welcome carries a roster")
            .iter()
            .all(|p| p.as_u64() != self_peer);
        assert_eq!(
            excluded,
            want.as_bool().expect("roster_excludes_self"),
            "{}: roster_excludes_self",
            case.label
        );
    });
    exp.assert_key_if_present("server_stamped_from", |want| {
        // A forwarded frame carries `from` (stamped by the server) and never a
        // client-supplied `to` — mixing both is the anti-spoof reject.
        let stamped = actual.get("from").is_some_and(|v| !v.is_null())
            && !actual.get("to").is_some_and(|v| !v.is_null());
        assert_eq!(
            stamped,
            want.as_bool().expect("server_stamped_from"),
            "{}: server_stamped_from",
            case.label
        );
    });
}

#[test]
fn anti_spoof_fixture_rejects_client_supplied_from() {
    let raw = common::spec_read_to_string(SESSION_PATH.path()).expect("read anti-spoof fixture");
    let fixture: SessionFixture =
        serde_json::from_str(&raw).expect("parse signaling anti-spoof fixture");
    assert_eq!(fixture.protocol_version, 1);
    assert_eq!(fixture.kind, "SignalingSession");
    assert_eq!(fixture.mode, "open");

    // These three were EXCUSED as "server-session claims the Rust crate cannot
    // reach", on the grounds that `lazily` ships the signalling client codec only
    // and the session model lives in `signaling/` (TypeScript). A corpus
    // perturbation pass showed what that cost (`#lznullformblind`): flipping any
    // of the three to `false` in a scratch copy of the corpus left this suite
    // GREEN, so the anti-spoof invariant the fixture exists for was one corpus
    // edit away from being switched off with nothing noticing. lazily-go found
    // the identical three keys from the other direction.
    //
    // Running the SERVER is indeed out of reach here. Asserting the claims is
    // not: the transcript's own emitted frames are ordinary signalling frames,
    // and this crate ships the decoder for them. Each claim below is checked
    // over those frames decoded through `ServerMessage` — the shipped codec, not
    // a re-read of the fixture — so it is grounded in the library rather than
    // fabricated. What stays out of scope is whether a server WOULD emit this
    // transcript; `signaling/test/protocol.test.ts` covers that.
    //
    // `conn -> peer` is the server-side registry: the peer id bound to a
    // connection when it joined. `forwarded_from_is_server_registered` is
    // exactly the claim that a forwarded frame's `from` equals the registry
    // entry for the connection that SENT it, never anything the sender wrote.
    let mut registry: BTreeMap<String, u64> = BTreeMap::new();
    let mut rosters_checked = 0usize;
    let mut forwarded_checked = 0usize;
    let mut emissions_bound = 0usize;
    let mut broadcast_steps_checked = 0usize;
    let mut roster_excludes_self = true;
    let mut roster_sorted = true;
    let mut from_is_registered = true;

    for (i, step) in fixture.steps.iter().enumerate() {
        let conn = step
            .input
            .conn
            .clone()
            .unwrap_or_else(|| panic!("step {i}: a session step names the sending connection"));
        // The inbound frame goes through the CLIENT codec, which is what binds
        // the registry to a decode rather than to the fixture's text.
        let recv = decode("client", &step.input.recv)
            .unwrap_or_else(|e| panic!("step {i}: session input should decode: {e}"));
        let input_type = recv["type"]
            .as_str()
            .unwrap_or_else(|| panic!("step {i}: a decoded input names its type"))
            .to_owned();
        // The roster a welcome must carry is the registry BEFORE this join, in
        // ascending peer order — so it has to be read here, ahead of the insert.
        let mut roster_before: Vec<u64> = registry.values().copied().collect();
        roster_before.sort_unstable();
        if input_type == "join" {
            registry.insert(
                conn.clone(),
                recv["peer"].as_u64().expect("a join names its peer"),
            );
        }
        // The peer id the SERVER has bound to this connection. Every `from` the
        // server stamps and every membership broadcast it sends about this step
        // names this value, and never anything the client wrote.
        let sender_peer = registry
            .get(&conn)
            .copied()
            .unwrap_or_else(|| panic!("step {i}: connection {conn:?} joined no session"));
        // The peer this input addressed, when it is directed, and the connection
        // registered for it. `None` for the second is the `unknown_target` case.
        let addressed = recv.get("to").and_then(Value::as_u64);
        let addressed_conn = addressed.and_then(|peer| {
            registry
                .iter()
                .find(|(_, registered)| **registered == peer)
                .map(|(target, _)| target.clone())
        });

        // The destinations this step's BROADCAST frames name. Collected so the
        // whole set can be compared against the roster in both directions after
        // the emissions — a per-frame membership check cannot see a peer the
        // server should have told and did not.
        let mut broadcast_to: BTreeSet<String> = BTreeSet::new();
        let mut step_broadcasts = false;

        for (j, emit) in step.expect.iter().enumerate() {
            // RUNG 0 (`#lzarrayelementsites`). Each element of `steps[n].expect`
            // is an assertion block: an expected emission carrying a routing
            // target and a frame. The walk in `tests/common/mod.rs` declares one
            // site per plain-object element, so each is bound here individually
            // and the label carries the index — a per-ARRAY label would collapse
            // the three emissions of step 2 into one name.
            let exp = Expect::new(SESSION_PATH, format!("steps[{i}].expect[{j}]"), emit);
            let declared = exp.get("frame");
            let frame = decode("server", declared)
                .unwrap_or_else(|e| panic!("step {i}: emitted frame should decode: {e}"));

            // THE KEY SET FIRST, in BOTH directions, before any value reaches a
            // comparison (`#lzsubblockkeyset`). The arms below used to read
            // `type`, `peer`, `peers` and `from` — only the keys the FIXTURE
            // happens to name — so a field the codec produced that the fixture
            // omits was compared by nothing. The descent below carries the
            // fixture-declares-it direction (an unconsumed sub-key fails); this
            // is the other one, and it names the offending key.
            let declared_keys: BTreeSet<&str> = declared
                .as_object()
                .unwrap_or_else(|| panic!("step {i} emission {j}: `frame` must be a JSON object"))
                .keys()
                .map(String::as_str)
                .collect();
            let produced_keys: BTreeSet<&str> = frame
                .as_object()
                .expect("a decoded server frame is a JSON object")
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(
                declared_keys,
                produced_keys,
                "step {i} emission {j}: the fixture's frame keys and the keys the \
                 codec produced disagree (#lzsubblockkeyset). Declared but not \
                 produced: {:?}; produced but not declared: {:?}.",
                declared_keys
                    .difference(&produced_keys)
                    .collect::<Vec<&&str>>(),
                produced_keys
                    .difference(&declared_keys)
                    .collect::<Vec<&&str>>(),
            );
            // ...then the frame's own bytes against the codec's, which is the
            // ROUND-TRIP claim and nothing more. It is stated separately from the
            // content claims below on purpose: `frame` decoded and re-serialised
            // is the fixture's own value, so this comparison is satisfied by any
            // transcript the codec can read and sees no corpus edit at all. It
            // was the whole of the frame's bind in the first draft here, and a
            // scratch-corpus probe that respelled an expected `sdp` stayed GREEN
            // under it — the exact `#lznullformblind` shape this family refuses.
            assert_eq!(
                frame, *declared,
                "step {i} emission {j}: the frame does not round-trip through the \
                 shipped server codec"
            );

            // THE CONTENT CLAIMS, every one of them derived from the DECODED
            // INPUT and the server-side registry, never from the frame's own
            // bytes. `Expect::sub` moves the key obligation down, so a frame key
            // no arm below consumes fails as an unconsumed key.
            let f = exp.sub("frame");
            match frame["type"].as_str() {
                Some("welcome") => {
                    let self_peer = frame["peer"].as_u64().expect("welcome names its peer");
                    let peers: Vec<u64> = frame["peers"]
                        .as_array()
                        .expect("welcome carries a roster")
                        .iter()
                        .map(|p| p.as_u64().expect("roster holds peer ids"))
                        .collect();
                    roster_excludes_self &= !peers.contains(&self_peer);
                    roster_sorted &= peers.windows(2).all(|w| w[0] < w[1]);
                    rosters_checked += 1;
                    assert_eq!(
                        input_type, "join",
                        "step {i}: a welcome answers a join and nothing else"
                    );
                    // A welcome answers the join that caused it, so it goes back
                    // to the connection that sent it and nowhere else.
                    exp.assert_key_at("to", conn.as_str(), &format!("step {i} welcome"));
                    f.assert_key("type", "welcome");
                    // The joining peer is the one the REGISTRY bound to this
                    // connection, and the roster is who was already in the
                    // session — which is `roster_excludes_self` and
                    // `roster_sorted_ascending` derived rather than read off the
                    // same frame they describe.
                    f.assert_key("peer", sender_peer);
                    f.assert_key("peers", roster_before.clone());
                }
                // A refusal answers the sender too — this arm used to be the
                // `_ => {}` fall-through, so step 6's whole emission reached
                // nothing at all.
                Some("error") => {
                    let addressed = addressed
                        .unwrap_or_else(|| panic!("step {i}: an error answers a directed input"));
                    assert!(
                        addressed_conn.is_none(),
                        "step {i}: the input addressed peer {addressed}, which IS \
                         registered — a routable frame must be forwarded, not refused"
                    );
                    exp.assert_key_at("to", conn.as_str(), &format!("step {i} error"));
                    f.assert_key("type", "error");
                    // The protocol's token for this condition, pinned by
                    // agreement rather than produced by a run — lazily-rs ships
                    // the codec, not the server that chooses the code. Unlike a
                    // self-comparison it is still falsifiable from the corpus
                    // side: respelling the fixture's code reddens here.
                    f.assert_key("code", "unknown_target");
                    f.assert_key_with("message", |want| {
                        let text = want.as_str().expect("an error message is prose");
                        assert!(
                            text.contains(&addressed.to_string()),
                            "step {i}: the refusal must name the peer the sender \
                             addressed ({addressed}); it says {text:?}"
                        );
                    });
                }
                Some(kind @ ("peer-joined" | "peer-left")) => {
                    let expected = if input_type == "join" {
                        "peer-joined"
                    } else {
                        "peer-left"
                    };
                    assert_eq!(
                        kind, expected,
                        "step {i}: an input of type {input_type:?} broadcasts \
                         {expected:?}"
                    );
                    step_broadcasts = true;
                    exp.assert_key_with("to", |want| {
                        let target = want.as_str().expect("a routing target names a connection");
                        assert!(
                            registry.contains_key(target),
                            "step {i}: a broadcast names connection {target:?}, which \
                             joined no session in this transcript"
                        );
                        assert_ne!(
                            target,
                            conn.as_str(),
                            "step {i}: a membership broadcast never returns to the \
                             connection that caused it"
                        );
                        broadcast_to.insert(target.to_owned());
                    });
                    f.assert_key("type", expected);
                    f.assert_key("peer", sender_peer);
                }
                // A forwarded frame is one carrying `from`. The server stamps it;
                // the sender never supplies it (the `rejects` half below is the
                // frame that tries).
                _ if frame.get("from").is_some_and(|v| !v.is_null()) => {
                    let stamped = frame["from"].as_u64().expect("`from` is a peer id");
                    from_is_registered &= registry.get(&conn) == Some(&stamped);
                    // ...and it is the REGISTRY's value, not an echo of whatever
                    // the input happened to carry.
                    from_is_registered &= step.input.recv.get("from").is_none();
                    forwarded_checked += 1;
                    // The other half of the routing claim: the frame is DELIVERED
                    // to the connection registered for the peer id the sender
                    // addressed. `from` says who it came from; `to` says the
                    // server resolved the target through the same registry.
                    let target = addressed_conn.clone().unwrap_or_else(|| {
                        panic!(
                            "step {i}: a forwarded frame whose input addressed no \
                             registered peer"
                        )
                    });
                    exp.assert_key_at("to", target.as_str(), &format!("step {i} forward"));
                    // A forward preserves the frame type and the body verbatim,
                    // replacing only the client's `to` with the server's `from`.
                    // Every remaining key is compared against the INPUT's value
                    // for the same key, so a respelled `sdp`, `candidate` or
                    // `payload` in the corpus reddens — the probe that caught
                    // the round-trip comparison above being vacuous.
                    f.assert_key("type", input_type.as_str());
                    f.assert_key("from", sender_peer);
                    for key in declared
                        .as_object()
                        .expect("a frame is an object")
                        .keys()
                        .filter(|key| *key != "type" && *key != "from")
                    {
                        f.assert_key_at(
                            key,
                            recv.get(key).cloned().unwrap_or(Value::Null),
                            &format!("step {i} forwarded body"),
                        );
                    }
                }
                other => panic!(
                    "step {i} emission {j}: frame type {other:?} reaches no routing rule. \
                     A new emitted shape must be classified, not silently ignored — the \
                     fall-through arm this replaces is what let the `error` emission go \
                     unexamined."
                ),
            }
            f.finish();
            emissions_bound += 1;
        }

        if step_broadcasts {
            // BOTH directions. A connection the server told that it should not
            // have, and a connection it should have told and did not, are both
            // failures; the per-frame membership check above sees only the first.
            let expected: BTreeSet<String> = registry
                .keys()
                .filter(|target| *target != &conn)
                .cloned()
                .collect();
            assert_eq!(
                broadcast_to, expected,
                "step {i}: the membership broadcast reached {broadcast_to:?}; every \
                 connection in the session except the sender {conn:?} is {expected:?}"
            );
            broadcast_steps_checked += 1;
        }
    }

    // "Exercised at least once" (`#lzvacuousrun`). A transcript-wide invariant
    // only fires where a frame of the right shape appears, so a match arm that
    // stopped recognising `welcome` — or a forwarded variant — would silence the
    // rule while every assertion below still read `true`.
    assert!(
        rosters_checked >= 2,
        "the roster rules were never asked: {rosters_checked} welcome frames reached the check"
    );
    assert!(
        forwarded_checked >= 3,
        "the anti-spoof rule this fixture exists for was never asked: \
         {forwarded_checked} forwarded frames reached the check"
    );
    assert!(
        broadcast_steps_checked >= 3,
        "the broadcast-fanout rule was never asked: {broadcast_steps_checked} steps \
         reached the set comparison"
    );
    // Every emission this transcript declares reached a bind AND a routing rule.
    // The block ledger pins the same 12 from the corpus side
    // (`#lzarrayelementsites`); this is the runner's own half, and it is what a
    // `continue` added to the loop above would fail.
    assert_eq!(
        emissions_bound, 12,
        "the transcript declares 12 emissions across its 8 steps; {emissions_bound} \
         reached the tracker"
    );

    let exp = Expect::new(SESSION_PATH, "assertions", &fixture.assertions);
    exp.assert_key("roster_excludes_self", roster_excludes_self);
    exp.assert_key("roster_sorted_ascending", roster_sorted);
    exp.assert_key("forwarded_from_is_server_registered", from_is_registered);

    for case in fixture.rejects {
        assert!(
            decode("client", &case.input.recv).is_err(),
            "{} should be rejected",
            case.label
        );
    }
}
