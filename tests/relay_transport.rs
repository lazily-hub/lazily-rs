//! Phase 4 spike tests for the `Transport` seam (`#relaycell`).
//!
//! Operational form of `LazilyFormal.Relay.transport_independent`: a `RelayCell`
//! fed through different transports (a direct `InProc` vs a `Framed` transport at
//! several frame sizes) converges to the same egress state — the merge algebra,
//! not the transport, guarantees convergence.

use lazily::{
    BackpressurePolicy, BoundDim, Context, FramedTransport, InProcTransport, KeepLatest, Max,
    MergeCellHandle, MergePolicy, Overflow, RelayCell, Sum, Transport,
};

/// Drive a relay by pumping `ops` through `transport`: deliver everything, then
/// poll frames, ingressing each op and draining the coalesced window per frame
/// into a downstream `MergeCell`. Returns the converged egress.
fn drive_through<T, M>(ctx: &Context, mut transport: T, ops: &[i64]) -> i64
where
    T: Transport<i64>,
    M: MergePolicy<i64> + 'static,
{
    let relay: RelayCell<i64, M> = RelayCell::new(
        ctx,
        BackpressurePolicy::new(ctx, BoundDim::Count, u64::MAX, 1, Overflow::Conflate),
    )
    .unwrap();
    let egress: MergeCellHandle<i64, M> = ctx.merge_cell(0);

    for &op in ops {
        transport.deliver(op);
    }
    while transport.has_pending() {
        let frame = transport.poll();
        for op in frame {
            relay.ingress(ctx, op);
        }
        // Flush the coalesced window at each transport frame boundary.
        if let Some(window) = relay.drain(ctx) {
            egress.merge(ctx, window);
        }
    }
    egress.get(ctx)
}

#[test]
fn converged_egress_independent_of_transport_framing() {
    let ctx = Context::new();
    let ops: Vec<i64> = vec![5, -3, 8, 2, -1, 7, 4, 6];
    let sum_flat: i64 = ops.iter().sum();
    let max_flat: i64 = ops.iter().copied().max().unwrap().max(0);
    let last = *ops.last().unwrap();

    // Direct transport reference.
    assert_eq!(
        drive_through::<_, Sum>(&ctx, InProcTransport::new(), &ops),
        sum_flat
    );

    // Framed transports at several MTUs — all converge identically.
    for frame in [1usize, 2, 3, 5, 100] {
        assert_eq!(
            drive_through::<_, Sum>(&ctx, FramedTransport::new(frame), &ops),
            sum_flat,
            "Sum frame {frame}"
        );
        assert_eq!(
            drive_through::<_, Max>(&ctx, FramedTransport::new(frame), &ops),
            max_flat,
            "Max frame {frame}"
        );
        assert_eq!(
            drive_through::<_, KeepLatest>(&ctx, FramedTransport::new(frame), &ops),
            last,
            "KeepLatest frame {frame}"
        );
    }
}

/// A framed transport delivers exactly the same ops in order — only the batching
/// differs. Frame size 1 = one op per frame; a huge frame = all at once.
#[test]
fn framed_transport_preserves_stream() {
    let mut t = FramedTransport::new(3);
    for v in [1, 2, 3, 4, 5, 6, 7] {
        t.deliver(v);
    }
    assert_eq!(t.poll(), vec![1, 2, 3]);
    assert_eq!(t.poll(), vec![4, 5, 6]);
    assert_eq!(t.poll(), vec![7]);
    assert!(!t.has_pending());
    assert_eq!(t.poll(), Vec::<i32>::new());

    let mut direct = InProcTransport::new();
    for v in [1, 2, 3] {
        direct.deliver(v);
    }
    assert_eq!(direct.poll(), vec![1, 2, 3]); // one frame
}
