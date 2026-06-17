//! Operator driver for the live two-host WebRTC e2e run (#h6qb / #lzwebrtcrunbook).
//!
//! Drives a real `Str0mNet` peer through the deployed `#yxjw` signaling Worker
//! so an operator can validate the networked backend across two real hosts /
//! NATs in one command per side. The localhost template is
//! `tests/webrtc_signaling.rs`; this binary is the same handshake opened up to
//! CLI args + a live Worker URL.
//!
//! See `tasks/software/runbook-lazily-webrtc-live-e2e.md` for the full procedure.
//!
//! # Usage
//!
//! Offerer (host A):
//! ```sh
//! cargo run --example webrtc_live_e2e --features "signaling-client webrtc-str0m" -- \
//!     wss://signaling.example.com session-1 1 2
//! ```
//! Answerer (host B):
//! ```sh
//! cargo run --example webrtc_live_e2e --features "signaling-client webrtc-str0m" -- \
//!     wss://signaling.example.com session-1 2
//! ```
//!
//! Positional args: `<signaling_url> <session> <peer_id> [offer_to_peer_id] [bind_addr]`.
//! The presence of `offer_to_peer_id` selects offerer mode; its absence selects
//! answerer mode (waits for any offer).

#![cfg(all(feature = "signaling-client", feature = "webrtc-str0m"))]

use std::net::SocketAddr;
use std::time::Duration;

use lazily::{
    IpcMessage, IpcSink, IpcSource, NodeId, NodeSnapshot, OpKind, PeerId, PeerPermissions,
    ServerMessage, SignalingClient, Snapshot, WebRtcSink, WebRtcSource, answer_next_offer,
    offer_to_peer,
};

const OPEN_TIMEOUT: Duration = Duration::from_secs(30);
const RECV_TIMEOUT: Duration = Duration::from_secs(30);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let signaling_url = args
        .next()
        .ok_or("usage: <signaling_url> <session> <peer_id> [offer_to_peer_id] [bind_addr]")?;
    let session = args
        .next()
        .ok_or("usage: <signaling_url> <session> <peer_id> [offer_to_peer_id] [bind_addr]")?;
    let peer: u64 = args
        .next()
        .ok_or("usage: <signaling_url> <session> <peer_id> [offer_to_peer_id] [bind_addr]")?
        .parse()?;
    let offer_to: Option<u64> = args.next().and_then(|s| s.parse().ok());
    let bind: SocketAddr = args
        .next()
        .map(|s| s.parse().expect("bind_addr"))
        .unwrap_or_else(|| "0.0.0.0:0".parse().expect("default bind"));

    eprintln!(
        "[lazily-e2e] connecting to {signaling_url}/session/{session} as peer {peer} (bind={bind})"
    );
    let mut sig = SignalingClient::connect(&signaling_url, &session, PeerId(peer)).await?;

    // Wait for the Worker's welcome so the roster is known before offering.
    let roster = loop {
        match sig.recv().await {
            Some(Ok(ServerMessage::Welcome { peers, .. })) => break peers,
            Some(Ok(_)) => continue,
            Some(Err(e)) => return Err(format!("signaling decode error: {e}").into()),
            None => return Err("signaling connection closed before welcome".into()),
        }
    };
    eprintln!("[lazily-e2e] joined; roster={roster:?}");

    let (remote_peer, net) = if let Some(target) = offer_to {
        eprintln!("[lazily-e2e] offerer: offering to peer {target}");
        (
            PeerId(target),
            offer_to_peer(&mut sig, PeerId(target), bind, OPEN_TIMEOUT).await?,
        )
    } else {
        eprintln!("[lazily-e2e] answerer: waiting for any offer");
        answer_next_offer(&mut sig, bind, OPEN_TIMEOUT).await?
    };
    eprintln!(
        "[lazily-e2e] data channel OPEN with peer {} (local_candidate={})",
        remote_peer.0,
        net.local_candidate()
    );

    // Verify the data path with a permission-filtered Snapshot round-trip —
    // the same shape as tests/str0m_net.rs and tests/webrtc_signaling.rs, so a
    // green run on two real hosts proves the live network path matches the
    // tested localhost path.
    if offer_to.is_some() {
        let mut perms = PeerPermissions::new();
        perms.allow_many(remote_peer, OpKind::Read, [NodeId(1)]);
        let mut sink = WebRtcSink::new(net.channel(), perms, remote_peer);
        let snapshot = Snapshot::new(
            1,
            vec![
                NodeSnapshot::payload(NodeId(1), "e2e-proof", b"live-roundtrip".to_vec()),
                NodeSnapshot::payload(NodeId(2), "filtered", b"should-not-arrive".to_vec()),
            ],
            vec![],
            vec![NodeId(1), NodeId(2)],
        );
        sink.send(&IpcMessage::Snapshot(snapshot))?;
        eprintln!(
            "[lazily-e2e] offerer sent permission-filtered snapshot (node 1 readable, node 2 denied)"
        );
        eprintln!(
            "[lazily-e2e] SUCCESS: offerer side complete. Answerer should print SUCCESS too."
        );
    } else {
        let mut source = WebRtcSource::new(net.channel());
        let deadline = tokio::time::Instant::now() + RECV_TIMEOUT;
        loop {
            if tokio::time::Instant::now() >= deadline {
                return Err("timed out waiting for snapshot from offerer".into());
            }
            if let Some(msg) = source.recv()?
                && let IpcMessage::Snapshot(s) = msg
            {
                let ids: Vec<u64> = s.nodes.iter().map(|n| n.node.0).collect();
                eprintln!("[lazily-e2e] answerer received snapshot nodes={ids:?} (expected [1])");
                if ids == vec![1] {
                    eprintln!("[lazily-e2e] SUCCESS: live round-trip + permission filter verified");
                    return Ok(());
                }
                return Err(format!(
                    "permission filter leaked: received nodes {ids:?}, expected [1]"
                )
                .into());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    Ok(())
}
