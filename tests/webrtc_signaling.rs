//! Full WebRTC handshake driven **through `SignalingClient`** over a loopback
//! signaling channel (#lzwebrtcwire).
//!
//! Unlike `tests/str0m_net.rs` — which exchanges the SDP offer/answer + ICE
//! candidates *in process* and only exercises the str0m UDP transport — this test
//! drives the offer/answer/ICE handshake over a **real WebSocket** to an
//! in-process signaling relay on `127.0.0.1`, using the production
//! `SignalingClient` and the `offer_to_peer` / `answer_next_offer` glue. The relay
//! implements the #yxjw wire protocol (roster + `from`-stamped routing) just
//! enough to broker two peers. The only remaining slice — two real hosts / NAT
//! through the deployed #yxjw Worker — is operator-gated (#h6qb).

#![cfg(all(feature = "signaling-client", feature = "webrtc-str0m"))]

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio_tungstenite::tungstenite::Message;

use lazily::{
    ClientMessage, IpcMessage, IpcSink, IpcSource, NodeId, NodeSnapshot, OpKind, PeerId,
    PeerPermissions, ServerMessage, SignalingClient, Snapshot, WebRtcSink, WebRtcSource,
    answer_next_offer, offer_to_peer,
};

/// Per-session roster: peer id -> a sender that writes frames to that peer's
/// WebSocket. Mirrors the #yxjw `SignalingRoom` roster.
type Roster = Arc<Mutex<HashMap<u64, UnboundedSender<Message>>>>;

/// Minimal loopback signaling relay implementing the #yxjw wire protocol for a
/// single session: roster on join, `peer-joined` broadcast, and `from`-stamped
/// routing of offer/answer/ice/relay. Spawns a task per accepted connection.
async fn run_relay(listener: TcpListener) {
    let roster: Roster = Arc::new(Mutex::new(HashMap::new()));
    while let Ok((stream, _addr)) = listener.accept().await {
        tokio::spawn(handle_conn(stream, roster.clone()));
    }
}

async fn handle_conn(stream: TcpStream, roster: Roster) {
    let ws = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(_) => return,
    };
    let (mut write, mut read) = ws.split();

    // First frame must be `join`; learn this connection's peer id.
    let peer = loop {
        match read.next().await {
            Some(Ok(Message::Text(t))) => match serde_json::from_str::<ClientMessage>(&t) {
                Ok(ClientMessage::Join { peer, .. }) => break peer,
                _ => continue,
            },
            Some(Ok(_)) => continue,
            _ => return,
        }
    };

    // Register, capturing the prior roster for the `welcome` frame.
    let (tx, mut rx) = unbounded_channel::<Message>();
    let others: Vec<PeerId> = {
        let mut guard = roster.lock().unwrap();
        let others = guard.keys().map(|id| PeerId(*id)).collect();
        guard.insert(peer.0, tx.clone());
        others
    };

    // Drive this connection's outbound queue to its socket on a side task.
    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if write.send(msg).await.is_err() {
                break;
            }
        }
    });

    send_to(
        &tx,
        &ServerMessage::Welcome {
            peer,
            peers: others,
        },
    );
    broadcast_peer_joined(&roster, peer);

    // Route inbound frames to their targets, `from`-stamped with this peer id.
    while let Some(Ok(msg)) = read.next().await {
        let Message::Text(text) = msg else { continue };
        let Ok(frame) = serde_json::from_str::<ClientMessage>(&text) else {
            continue;
        };
        match frame {
            ClientMessage::Offer { to, sdp } => {
                route(&roster, to, &ServerMessage::Offer { from: peer, sdp })
            }
            ClientMessage::Answer { to, sdp } => {
                route(&roster, to, &ServerMessage::Answer { from: peer, sdp })
            }
            ClientMessage::Ice { to, candidate } => route(
                &roster,
                to,
                &ServerMessage::Ice {
                    from: peer,
                    candidate,
                },
            ),
            ClientMessage::Relay { to, payload } => route(
                &roster,
                to,
                &ServerMessage::Relay {
                    from: peer,
                    payload,
                },
            ),
            ClientMessage::Leave => break,
            ClientMessage::Join { .. } => {}
        }
    }

    roster.lock().unwrap().remove(&peer.0);
    writer.abort();
}

fn send_to(tx: &UnboundedSender<Message>, msg: &ServerMessage) {
    if let Ok(json) = serde_json::to_string(msg) {
        let _ = tx.send(Message::Text(json));
    }
}

fn route(roster: &Roster, to: PeerId, msg: &ServerMessage) {
    let tx = roster.lock().unwrap().get(&to.0).cloned();
    if let Some(tx) = tx {
        send_to(&tx, msg);
    }
}

fn broadcast_peer_joined(roster: &Roster, joined: PeerId) {
    let targets: Vec<UnboundedSender<Message>> = {
        let guard = roster.lock().unwrap();
        guard
            .iter()
            .filter(|(id, _)| **id != joined.0)
            .map(|(_, tx)| tx.clone())
            .collect()
    };
    for tx in targets {
        send_to(&tx, &ServerMessage::PeerJoined { peer: joined });
    }
}

/// Receive the next frame from a client, asserting it is present.
async fn expect_msg(client: &mut SignalingClient) -> ServerMessage {
    client
        .recv()
        .await
        .expect("signaling connection stayed open")
        .expect("signaling frame decoded")
}

#[tokio::test]
async fn handshake_through_signaling_client_carries_filtered_snapshot() {
    // 1. Stand up the loopback signaling relay on an ephemeral port.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("ws://{}", listener.local_addr().unwrap());
    tokio::spawn(run_relay(listener));

    // 2. Two peers join the same session over real WebSockets.
    let mut offerer_sig = SignalingClient::connect(&base, "room-1", PeerId(1))
        .await
        .expect("offerer joins");
    assert!(matches!(
        expect_msg(&mut offerer_sig).await,
        ServerMessage::Welcome {
            peer: PeerId(1),
            ..
        }
    ));

    let mut answerer_sig = SignalingClient::connect(&base, "room-1", PeerId(2))
        .await
        .expect("answerer joins");
    match expect_msg(&mut answerer_sig).await {
        // The answerer's welcome roster must already list the offerer.
        ServerMessage::Welcome { peer, peers } => {
            assert_eq!(peer, PeerId(2));
            assert_eq!(peers, vec![PeerId(1)]);
        }
        other => panic!("expected welcome, got {other:?}"),
    }
    // The offerer learns the answerer is present before it offers (this also
    // guarantees the relay has registered peer 2, so the offer cannot race ahead
    // of its roster entry).
    assert!(matches!(
        expect_msg(&mut offerer_sig).await,
        ServerMessage::PeerJoined { peer: PeerId(2) }
    ));

    // 3. Drive both sides of the handshake concurrently THROUGH SignalingClient.
    let bind: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let offer_task = tokio::spawn(async move {
        offer_to_peer(&mut offerer_sig, PeerId(2), bind, Duration::from_secs(20))
            .await
            .expect("offerer opens data channel")
    });
    let answer_task = tokio::spawn(async move {
        answer_next_offer(&mut answerer_sig, bind, Duration::from_secs(20))
            .await
            .expect("answerer opens data channel")
    });

    let offerer = offer_task.await.expect("offer task");
    let (peer, answerer) = answer_task.await.expect("answer task");
    assert_eq!(peer, PeerId(1), "answerer learns the offering peer id");

    assert!(offerer.is_open(), "offerer data channel open");
    assert!(answerer.is_open(), "answerer data channel open");

    // 4. The negotiated channel carries a permission-filtered Snapshot, proving
    //    the str0m transport is live end-to-end after signaling.
    let reader = PeerId(2);
    let mut perms = PeerPermissions::new();
    perms.allow_many(reader, OpKind::Read, [NodeId(1)]);

    let mut sink = WebRtcSink::new(offerer.channel(), perms, reader);
    let mut source = WebRtcSource::new(answerer.channel());

    let snapshot = Snapshot::new(
        1,
        vec![
            NodeSnapshot::payload(NodeId(1), "t", vec![1, 2, 3]),
            NodeSnapshot::payload(NodeId(2), "t", vec![4, 5, 6]),
        ],
        vec![],
        vec![NodeId(1), NodeId(2)],
    );
    sink.send(&IpcMessage::Snapshot(snapshot)).expect("send");

    let mut got = None;
    for _ in 0..200 {
        if let Some(msg) = source.recv().expect("recv") {
            got = Some(msg);
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    match got.expect("snapshot arrives over the signaled data channel") {
        IpcMessage::Snapshot(s) => {
            let ids: Vec<u64> = s.nodes.iter().map(|n| n.node.0).collect();
            assert_eq!(ids, vec![1], "node 2 must be filtered out for this peer");
        }
        other => panic!("expected snapshot, got {other:?}"),
    }
}
