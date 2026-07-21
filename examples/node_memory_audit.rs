//! Where the per-subscriber bytes actually go (#lzspecedgeindex, step 1).
//!
//! ```sh
//! RUSTFLAGS="--cfg audit_probe" cargo run --release --example node_memory_audit
//! LAZILY_AUDIT_MAX_WIDTH=10000000 RUSTFLAGS="--cfg audit_probe" \
//!   cargo run --release --example node_memory_audit
//! ```
//!
//! `examples/pubsub_load.rs` reports a flat ~200-247 bytes per subscriber from
//! RSS deltas. RSS is a true total but an opaque one: it cannot say whether a
//! byte is a node field, a closure box, an edge slot, capacity slack the
//! allocator will never hand back, or page rounding. This answers that with
//! counts and sizes instead, which are load-immune — nothing here is timed, so
//! nothing here degrades when the box is busy.
//!
//! Three questions, in order:
//!
//! 1. **Static layout.** `size_of` for every type in a node. The arena stores
//!    `Option<Node>`, an enum sized by its largest variant, so every cell pays
//!    the slot's width. That tax is knowable before anything runs.
//! 2. **Counted allocations.** A `#[global_allocator]` wrapper tallies every
//!    `alloc`/`dealloc` by exact size, so per-subscriber cost is a measured
//!    delta rather than an inferred one: how many distinct heap blocks a node
//!    spans, and how wide each is.
//! 3. **Marginal vs fixed.** Each rung reports the slope against the previous,
//!    separating cost that scales with width from one-time context overhead
//!    and from `Vec` capacity slack (which doubles, so it is sawtoothed and
//!    width-dependent in a way payload is not).
//!
//! The build is split into phases (context, node creation, edge registration)
//! so the total is attributed rather than lumped. Two value types are measured:
//! `u64`, which `AnyValue` stores inline, and `String`, which it cannot — the
//! delta between them isolates the value box exactly.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering::Relaxed};

use lazily::Context;

// ---------------------------------------------------------------------------
// Counting allocator
// ---------------------------------------------------------------------------

/// Exact-size tracking up to this many bytes; larger blocks fall into
/// power-of-two buckets. Every per-node allocation in this crate is far below
/// it, so the interesting histogram is exact.
const SMALL_MAX: usize = 1024;

static ALLOC_CALLS: AtomicU64 = AtomicU64::new(0);
static DEALLOC_CALLS: AtomicU64 = AtomicU64::new(0);
static LIVE_BYTES: AtomicI64 = AtomicI64::new(0);
static LIVE_BLOCKS: AtomicI64 = AtomicI64::new(0);
/// Net live block count per exact size, for sizes <= SMALL_MAX.
static SMALL_HIST: [AtomicI64; SMALL_MAX + 1] = [const { AtomicI64::new(0) }; SMALL_MAX + 1];
/// Net live block count per power-of-two bucket, for sizes > SMALL_MAX.
static LARGE_HIST: [AtomicI64; 64] = [const { AtomicI64::new(0) }; 64];
/// Net live *bytes* per power-of-two bucket. The bucket index only floors the
/// size, so the exact byte total has to be carried alongside the count —
/// otherwise a 160 MiB arena reports as 128 MiB and the breakdown stops summing.
static LARGE_BYTES: [AtomicI64; 64] = [const { AtomicI64::new(0) }; 64];

fn record_alloc(size: usize) {
    ALLOC_CALLS.fetch_add(1, Relaxed);
    LIVE_BYTES.fetch_add(size as i64, Relaxed);
    LIVE_BLOCKS.fetch_add(1, Relaxed);
    if size <= SMALL_MAX {
        SMALL_HIST[size].fetch_add(1, Relaxed);
    } else {
        let bucket = usize::BITS as usize - 1 - size.leading_zeros() as usize;
        LARGE_HIST[bucket].fetch_add(1, Relaxed);
        LARGE_BYTES[bucket].fetch_add(size as i64, Relaxed);
    }
}

fn record_dealloc(size: usize) {
    DEALLOC_CALLS.fetch_add(1, Relaxed);
    LIVE_BYTES.fetch_sub(size as i64, Relaxed);
    LIVE_BLOCKS.fetch_sub(1, Relaxed);
    if size <= SMALL_MAX {
        SMALL_HIST[size].fetch_sub(1, Relaxed);
    } else {
        let bucket = usize::BITS as usize - 1 - size.leading_zeros() as usize;
        LARGE_HIST[bucket].fetch_sub(1, Relaxed);
        LARGE_BYTES[bucket].fetch_sub(size as i64, Relaxed);
    }
}

struct Counting;

// SAFETY: every method forwards to `System` with the same layout it was given;
// the counters are side effects that never touch the returned pointers.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            record_alloc(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        record_dealloc(layout.size());
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            record_alloc(layout.size());
        }
        ptr
    }

    // Delegated rather than left to the trait default (alloc + copy + dealloc)
    // so growing a 10M-element arena stays one `mremap` instead of a full copy.
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let out = unsafe { System.realloc(ptr, layout, new_size) };
        if !out.is_null() {
            record_dealloc(layout.size());
            record_alloc(new_size);
        }
        out
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

#[derive(Clone, Copy, Default)]
struct Snapshot {
    alloc_calls: u64,
    dealloc_calls: u64,
    live_bytes: i64,
    live_blocks: i64,
}

fn snapshot() -> Snapshot {
    Snapshot {
        alloc_calls: ALLOC_CALLS.load(Relaxed),
        dealloc_calls: DEALLOC_CALLS.load(Relaxed),
        live_bytes: LIVE_BYTES.load(Relaxed),
        live_blocks: LIVE_BLOCKS.load(Relaxed),
    }
}

impl Snapshot {
    fn since(self, base: Snapshot) -> Snapshot {
        Snapshot {
            alloc_calls: self.alloc_calls - base.alloc_calls,
            dealloc_calls: self.dealloc_calls - base.dealloc_calls,
            live_bytes: self.live_bytes - base.live_bytes,
            live_blocks: self.live_blocks - base.live_blocks,
        }
    }
}

/// Fixed-width net-live histogram: exact sizes `0..=SMALL_MAX`, then one
/// power-of-two bucket per larger size.
///
/// Deliberately an array and not a `Vec`: this is captured at every phase
/// boundary inside the measured region, and a heap-allocated snapshot would
/// land in the very counters it is snapshotting.
type Hist = [i64; SMALL_MAX + 1 + 128];

fn hist_capture() -> Hist {
    let mut hist = [0i64; SMALL_MAX + 1 + 128];
    for (size, cell) in hist.iter_mut().enumerate().take(SMALL_MAX + 1) {
        *cell = SMALL_HIST[size].load(Relaxed);
    }
    for bucket in 0..64 {
        hist[SMALL_MAX + 1 + bucket] = LARGE_HIST[bucket].load(Relaxed);
        hist[SMALL_MAX + 1 + 64 + bucket] = LARGE_BYTES[bucket].load(Relaxed);
    }
    hist
}

/// One live-block class: how many blocks, how many bytes in total, and their
/// mean size. Small classes are exact sizes; large ones are a power-of-two
/// bucket whose byte total is nevertheless exact.
struct HistRow {
    blocks: i64,
    bytes: i64,
    exact_size: bool,
}

/// Live-block classes for `hist - base`, largest total bytes first.
fn hist_rows(hist: &Hist, base: &Hist) -> Vec<HistRow> {
    let mut rows: Vec<HistRow> = (0..=SMALL_MAX)
        .filter_map(|size| {
            let blocks = hist[size] - base[size];
            (blocks != 0).then_some(HistRow {
                blocks,
                bytes: blocks * size as i64,
                exact_size: true,
            })
        })
        .collect();
    // Buckets at or below SMALL_MAX are structurally empty — small sizes are
    // tracked exactly — so only the genuinely large ones are worth a row.
    rows.extend((11..64).filter_map(|bucket| {
        let blocks = hist[SMALL_MAX + 1 + bucket] - base[SMALL_MAX + 1 + bucket];
        let bytes = hist[SMALL_MAX + 1 + 64 + bucket] - base[SMALL_MAX + 1 + 64 + bucket];
        (blocks != 0 || bytes != 0).then_some(HistRow {
            blocks,
            bytes,
            exact_size: false,
        })
    }));
    rows.sort_by_key(|row| std::cmp::Reverse(row.bytes));
    rows
}

fn rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status.lines().find_map(|line| {
                line.strip_prefix("VmRSS:")?
                    .split_whitespace()
                    .next()?
                    .parse()
                    .ok()
            })
        })
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Phase-split build
// ---------------------------------------------------------------------------

struct WidthReport {
    width: usize,
    ctx_only: Snapshot,
    after_create: Snapshot,
    after_register: Snapshot,
    rss_bytes_each: f64,
    /// Per-phase live-block histograms, so a block is attributed to the phase
    /// that created it rather than read out of a running total.
    phase_histogram_a: [Vec<HistRow>; 3],
}

/// Build a width-N topic in three observable phases and report the counted
/// allocations attributable to each.
///
/// Phase 1 is the empty `Context` plus the topic cell — the fixed overhead that
/// does not scale. Phase 2 creates N computed slots without reading them, so it
/// isolates arena growth and compute-closure boxes. Phase 3 reads every slot,
/// which is what registers the dependency edges: the topic's dependent list and
/// (past `EDGE_INDEX_THRESHOLD`) its hash index.
fn sweep_width(width: usize) -> WidthReport {
    // The harness's own handle vector is allocated before the baseline, so it
    // never enters the library's per-node totals. Subtracting it afterwards
    // would fix the byte sum but still leave a phantom `width * 8` block in the
    // per-phase histogram.
    let mut subscriber_a: Vec<_> = Vec::with_capacity(width);
    let rss_before = rss_kib();
    let base = snapshot();

    let hist_base = hist_capture();

    let ctx = Context::new();
    let topic = ctx.cell(0u64);
    let ctx_only = snapshot().since(base);
    let hist_ctx = hist_capture();

    subscriber_a.extend((0..width).map(|i| ctx.computed(move |ctx| ctx.get(&topic) + i as u64)));
    let after_create = snapshot().since(base);
    let hist_create = hist_capture();

    for slot in &subscriber_a {
        std::hint::black_box(ctx.get(slot));
    }
    let after_register = snapshot().since(base);
    let hist_register = hist_capture();

    let rss_bytes_each =
        (rss_kib().saturating_sub(rss_before)) as f64 * 1024.0 / width.max(1) as f64;

    let report = WidthReport {
        width,
        ctx_only,
        after_create,
        after_register,
        rss_bytes_each,
        phase_histogram_a: [
            hist_rows(&hist_ctx, &hist_base),
            hist_rows(&hist_create, &hist_ctx),
            hist_rows(&hist_register, &hist_create),
        ],
    };
    drop(subscriber_a);
    drop(ctx);
    report
}

fn size_of_handle() -> usize {
    std::mem::size_of::<lazily::Computed<u64>>()
}

fn print_width_report(report: &WidthReport, previous: Option<&WidthReport>) {
    let width = report.width as f64;
    let total = report.after_register;
    println!(
        "\nwidth {} — {} live blocks, {} live bytes",
        report.width, total.live_blocks, total.live_bytes,
    );
    println!(
        "  {:<28}{:>14}{:>14}{:>14}",
        "phase", "live blocks", "live bytes", "bytes/sub"
    );
    let phases = [
        ("1 context + topic cell", report.ctx_only),
        ("2 +N computed (unread)", report.after_create),
        ("3 +N edges registered", report.after_register),
    ];
    let mut previous_phase = Snapshot::default();
    for (name, phase) in phases {
        println!(
            "  {:<28}{:>14}{:>14}{:>14.2}",
            name,
            phase.live_blocks - previous_phase.live_blocks,
            phase.live_bytes - previous_phase.live_bytes,
            (phase.live_bytes - previous_phase.live_bytes) as f64 / width,
        );
        previous_phase = phase;
    }
    println!(
        "  {:<28}{:>14}{:>14}{:>14.2}",
        "TOTAL",
        total.live_blocks,
        total.live_bytes,
        total.live_bytes as f64 / width,
    );
    println!(
        "  counted {:.2} B/sub, {:.3} live blocks/sub | RSS {:.2} B/sub \
         (RSS - counted = allocator metadata + page rounding)",
        total.live_bytes as f64 / width,
        total.live_blocks as f64 / width,
        report.rss_bytes_each,
    );
    println!("  {:.2} alloc calls/sub", total.alloc_calls as f64 / width);

    if let Some(previous) = previous {
        let delta_width = (report.width - previous.width) as f64;
        println!(
            "  marginal vs width {}: {:.2} B/sub, {:.3} blocks/sub \
             (slope = what actually scales; total - slope*width = fixed)",
            previous.width,
            (total.live_bytes - previous.after_register.live_bytes) as f64 / delta_width,
            (total.live_blocks - previous.after_register.live_blocks) as f64 / delta_width,
        );
    }

    let phase_name_a = [
        "1 context + topic cell",
        "2 +N computed (unread)",
        "3 +N edges registered",
    ];
    for (name, histogram) in phase_name_a.iter().zip(report.phase_histogram_a.iter()) {
        println!("  live blocks created by phase {name} (top 5):");
        for row in histogram.iter().take(5) {
            let mean = if row.blocks == 0 {
                0
            } else {
                row.bytes / row.blocks
            };
            println!(
                "    {:>12} B {:<7} x {:>9} blocks = {:>14} B ({:>8.2} B/sub)",
                mean,
                if row.exact_size { "exact" } else { "mean" },
                row.blocks,
                row.bytes,
                row.bytes as f64 / width,
            );
        }
    }
}

/// Same shape, but with a value type `AnyValue` cannot store inline. The delta
/// against the `u64` ladder is the value box, isolated.
fn sweep_heap_valued(width: usize) -> (i64, i64) {
    let mut subscriber_a: Vec<_> = Vec::with_capacity(width);
    let base = snapshot();
    let ctx = Context::new();
    let topic = ctx.cell(0u64);
    subscriber_a
        .extend((0..width).map(|i| ctx.computed(move |ctx| format!("{}-{}", ctx.get(&topic), i))));
    for slot in &subscriber_a {
        std::hint::black_box(ctx.get(slot));
    }
    let total = snapshot().since(base);
    drop(subscriber_a);
    drop(ctx);
    (total.live_bytes, total.live_blocks)
}

fn main() {
    println!("=== 1. static layout (size_of / align_of) ===\n");
    #[cfg(audit_probe)]
    {
        println!("  {:<44}{:>8}{:>8}", "type", "size", "align");
        for (name, size, align) in lazily::audit_probe::layout_rows() {
            println!("  {name:<44}{size:>8}{align:>8}");
        }
        println!("\n  ComputedNode field offsets (gaps are padding):");
        let mut offsets = lazily::audit_probe::slot_node_field_offsets();
        offsets.sort_by_key(|(_, offset)| *offset);
        for (name, offset) in &offsets {
            println!("    {name:<20} @ {offset:>4}  (cache line {})", offset / 64);
        }

        // The invalidation walk (`mark_frontier_locked`) reads the node
        // discriminant, reads and writes `dirty`/`force_recompute`, and reads
        // `dependents` to recurse. It never dereferences the compute closure.
        // So the marking cost is not "how many allocations" — it is how far
        // apart, inside one node, those few bits sit.
        let mark_fields = ["dependents", "dirty", "force_recompute"];
        let mut lines: Vec<usize> = offsets
            .iter()
            .filter(|(name, _)| mark_fields.contains(name))
            .map(|(_, offset)| offset / 64)
            .collect();
        lines.sort_unstable();
        lines.dedup();
        let node_size = lazily::audit_probe::layout_rows()
            .into_iter()
            .find(|(name, _, _)| *name == "Option<super::Node>")
            .map_or(0, |(_, size, _)| size);
        println!();
        println!("  invalidation (mark_frontier_locked) sets 2 bits of state per node.");
        println!("  To reach them it strides {node_size} B of arena per node and touches");
        println!(
            "  {} distinct 64 B cache line(s) {:?} inside each node.",
            lines.len(),
            lines,
        );
    }
    #[cfg(not(audit_probe))]
    println!(
        "  (skipped: rerun with RUSTFLAGS=\"--cfg audit_probe\" for the \
         internal type layout)"
    );

    println!(
        "\n  Computed<u64> (harness-side, not library memory): {} B",
        size_of_handle()
    );

    println!("\n=== 2 + 3. counted allocations per subscriber ===");
    let max_width: usize = std::env::var("LAZILY_AUDIT_MAX_WIDTH")
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(1_000_000);
    // Straddles EDGE_INDEX_THRESHOLD (32) so the index's contribution appears
    // as a step, and climbs far enough that Vec capacity slack can be told
    // apart from payload (slack is sawtoothed across a doubling, payload is not).
    let ladder = [16usize, 32, 33, 64, 128, 1024, 4096, 65_536, 1_000_000];
    // Absorb one-time process allocations (stdout buffer, lazily-initialized
    // statics) into a discarded rung, so the first real rung is not charged for
    // them and the narrow widths stay readable.
    std::hint::black_box(sweep_width(64));
    let mut previous: Option<WidthReport> = None;
    for width in ladder {
        if width > max_width {
            break;
        }
        let report = sweep_width(width);
        print_width_report(&report, previous.as_ref());
        previous = Some(report);
    }

    println!("\n=== 4. inline vs heap value storage ===\n");
    let probe_width = 4096.min(max_width);
    let inline = sweep_width(probe_width);
    let (heap_bytes, heap_blocks) = sweep_heap_valued(probe_width);
    let each = probe_width as f64;
    println!(
        "  width {probe_width}: u64 (inline)  {:>10} B, {:>8} blocks -> {:.2} B/sub, {:.3} blocks/sub",
        inline.after_register.live_bytes,
        inline.after_register.live_blocks,
        inline.after_register.live_bytes as f64 / each,
        inline.after_register.live_blocks as f64 / each,
    );
    println!(
        "  width {probe_width}: String (heap) {:>10} B, {:>8} blocks -> {:.2} B/sub, {:.3} blocks/sub",
        heap_bytes,
        heap_blocks,
        heap_bytes as f64 / each,
        heap_blocks as f64 / each,
    );
    println!(
        "  value-box cost when not inline-eligible: {:+.2} B/sub, {:+.3} blocks/sub",
        (heap_bytes - inline.after_register.live_bytes) as f64 / each,
        (heap_blocks - inline.after_register.live_blocks) as f64 / each,
    );

    let leaked = snapshot();
    println!(
        "\n=== leak check: {} live blocks / {} live bytes still held \
         (harness Vecs and process statics only) ===",
        leaked.live_blocks, leaked.live_bytes,
    );
}
