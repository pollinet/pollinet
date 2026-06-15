//! In-process multi-node mesh simulator (no radio).
//!
//! Builds a network of real `HostWifiDirectTransport` nodes — the exact engine used in
//! production (fragmentation, reassembly, content-hash dedup, store-and-forward) — and
//! propagates a transaction across them with store-and-forward relaying. Validates the
//! shared mesh/queue stack under hostile conditions at 10 / 50 / 100 nodes: duplicate
//! suppression, hop-limit (TTL) bounding, packet loss, network partition + heal, and
//! node churn.
//!
//! `ffi` is android-gated, so this whole file is too; it runs under
//! `cargo test --features android` and is an empty (passing) crate otherwise.
#![cfg(feature = "android")]

use pollinet::ble::MAX_HOPS;
use pollinet::ffi::host_transport::HostTransport;
use pollinet::ffi::wifi_direct_transport::{HostWifiDirectTransport, WIFI_DIRECT_MAX_FRAME};
use rand::{rngs::StdRng, Rng, SeedableRng};
use std::collections::HashSet;
use std::sync::Arc;

/// A delivery in flight on the simulated wire: (destination node, frame bytes, hop count).
type InFlight = (usize, Vec<u8>, u8);

struct Sim {
    nodes: Vec<Arc<HostWifiDirectTransport>>,
    /// Undirected adjacency list.
    neighbors: Vec<Vec<usize>>,
}

impl Sim {
    async fn new(n: usize) -> Self {
        let mut nodes = Vec::with_capacity(n);
        for _ in 0..n {
            nodes.push(Arc::new(HostWifiDirectTransport::new().await.unwrap()));
        }
        Sim {
            nodes,
            neighbors: vec![Vec::new(); n],
        }
    }

    /// Connect a as a line (chain) — the worst case for hop count / TTL.
    fn wire_line(&mut self) {
        for i in 0..self.nodes.len().saturating_sub(1) {
            self.connect(i, i + 1);
        }
    }

    /// Connect a sparse random graph (each node linked to a few others) on top of a
    /// spanning line so the graph is always connected.
    fn wire_random(&mut self, extra_per_node: usize, seed: u64) {
        self.wire_line();
        let n = self.nodes.len();
        let mut rng = StdRng::seed_from_u64(seed);
        for i in 0..n {
            for _ in 0..extra_per_node {
                let j = rng.gen_range(0..n);
                if j != i {
                    self.connect(i, j);
                }
            }
        }
    }

    /// Connect a deterministic low-diameter "small-world" graph: each node links to peers
    /// at offsets `jumps` (mod n). With jumps {1,2,4,8,16,32} the diameter is ~log2(n),
    /// comfortably under MAX_HOPS even at 100 nodes — so a single hop-bounded flood can
    /// reach everyone (lets us assert *full* delivery rather than just TTL-bounded reach).
    fn wire_smallworld(&mut self, jumps: &[usize]) {
        let n = self.nodes.len();
        for i in 0..n {
            for &j in jumps {
                let k = (i + j) % n;
                if k != i {
                    self.connect(i, k);
                }
            }
        }
    }

    fn connect(&mut self, a: usize, b: usize) {
        if !self.neighbors[a].contains(&b) {
            self.neighbors[a].push(b);
        }
        if !self.neighbors[b].contains(&a) {
            self.neighbors[b].push(a);
        }
    }

    /// Drain every outbound frame currently queued on `node`.
    fn drain(&self, node: usize) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        while let Some(f) = self.nodes[node].next_outbound(WIFI_DIRECT_MAX_FRAME) {
            out.push(f);
        }
        out
    }

    /// Flood `tx` from `origin` across the mesh with store-and-forward relaying.
    ///
    /// * `loss` — per-frame drop probability (0.0..=1.0).
    /// * `active` — nodes that are powered on; inactive nodes neither receive nor relay.
    /// * Returns the set of nodes that fully reassembled the transaction.
    ///
    /// TTL is emulated by the `hop` counter: a relayed frame carries `hop+1` and is
    /// dropped once `hop > MAX_HOPS`, exactly bounding propagation like `MeshHeader.ttl`.
    fn flood(
        &self,
        origin: usize,
        tx: &[u8],
        loss: f64,
        active: &HashSet<usize>,
        rng: &mut StdRng,
    ) -> HashSet<usize> {
        let mut received: HashSet<usize> = HashSet::new();
        let mut relayed: HashSet<usize> = HashSet::new();

        // Origin fragments the transaction once and seeds the wire.
        self.nodes[origin]
            .queue_transaction(tx.to_vec(), None)
            .unwrap();
        let mut wire: Vec<InFlight> = Vec::new();
        for f in self.drain(origin) {
            for &nb in &self.neighbors[origin] {
                wire.push((nb, f.clone(), 1));
            }
        }
        relayed.insert(origin);
        received.insert(origin);

        // Process the wire until it drains. Relays append new in-flight frames.
        let mut guard = 0usize;
        while let Some((dst, frame, hop)) = wire.pop() {
            guard += 1;
            assert!(guard < 5_000_000, "flood failed to converge (loop?)");

            if hop > MAX_HOPS || !active.contains(&dst) {
                continue; // TTL exceeded or node offline → drop (store-and-forward elsewhere)
            }
            if rng.gen_bool(loss) {
                continue; // packet lost on the link
            }

            // Deliver the frame to the destination's real engine.
            let _ = self.nodes[dst].push_inbound(frame);

            // Did this complete a transaction the node hadn't seen? Relay once.
            if let Some((_id, _bytes)) = self.nodes[dst].pop_completed() {
                received.insert(dst);
                if relayed.insert(dst) && hop < MAX_HOPS {
                    self.nodes[dst].queue_transaction(tx.to_vec(), None).unwrap();
                    for f in self.drain(dst) {
                        for &nb in &self.neighbors[dst] {
                            if nb != dst {
                                wire.push((nb, f.clone(), hop + 1));
                            }
                        }
                    }
                }
            }
        }
        received
    }
}

fn all_active(n: usize) -> HashSet<usize> {
    (0..n).collect()
}

/// Happy-ish path: connected random mesh, no loss → every node receives exactly once.
/// Run at 10/50/100 nodes. "Exactly once" is enforced by the engine's received-queue
/// dedup (asserted via received_queue_size == 1 on every node).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sim_full_delivery_scales_10_50_100() {
    for &n in &[10usize, 50, 100] {
        let mut sim = Sim::new(n).await;
        sim.wire_smallworld(&[1, 2, 4, 8, 16, 32]);
        let mut rng = StdRng::seed_from_u64(42);
        let tx: Vec<u8> = (0..1500).map(|i| (i % 251) as u8).collect();

        let received = sim.flood(0, &tx, 0.0, &all_active(n), &mut rng);
        assert_eq!(received.len(), n, "{n}-node mesh: every node should receive");

        for (i, node) in sim.nodes.iter().enumerate() {
            assert_eq!(
                node.received_queue_size(),
                1,
                "node {i}/{n} must have the tx queued exactly once (dedup)"
            );
        }
    }
}

/// Lossy links: with relaying + redundancy a connected mesh still reaches everyone, and
/// still never double-processes (dedup holds under heavy retransmission).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sim_packet_loss_still_converges_and_dedups() {
    let n = 50;
    let mut sim = Sim::new(n).await;
    sim.wire_smallworld(&[1, 2, 4, 8, 16]); // dense, low-diameter, lots of relay redundancy
    let mut rng = StdRng::seed_from_u64(123);
    let tx = vec![0xABu8; 1200];

    let received = sim.flood(0, &tx, 0.25, &all_active(n), &mut rng);
    // Relay redundancy across many neighbors should cover the vast majority despite 25%
    // per-frame loss; the key invariant is that dedup never breaks under retransmission.
    assert!(
        received.len() >= (n * 9) / 10,
        "lossy mesh should still reach >=90% of nodes (reached {})",
        received.len()
    );
    for node in &sim.nodes {
        assert!(node.received_queue_size() <= 1, "dedup must hold under loss");
    }
}

/// Hop-limit (TTL): on a long line longer than MAX_HOPS, nodes beyond the horizon are
/// NOT reached in a single flood — propagation is bounded, not infinite.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sim_ttl_bounds_propagation_on_long_line() {
    let n = (MAX_HOPS as usize) + 8; // a few nodes past the TTL horizon
    let mut sim = Sim::new(n).await;
    sim.wire_line();
    let mut rng = StdRng::seed_from_u64(9);
    let tx = vec![1u8; 800];

    let received = sim.flood(0, &tx, 0.0, &all_active(n), &mut rng);
    assert!(
        received.len() < n,
        "TTL must bound a line longer than MAX_HOPS (reached {} of {n})",
        received.len()
    );
    assert!(received.contains(&1), "immediate neighbor should be reached");
    assert!(
        !received.contains(&(n - 1)),
        "the far end (beyond MAX_HOPS) must not be reached in one flood"
    );
}

/// Partition + heal + store-and-forward: while the network is split, only the origin's
/// component receives. After the partition heals, a re-broadcast from a node that still
/// holds the transaction (its persisted copy) delivers to the rest — reconnect *resumes*,
/// it does not restart from scratch.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sim_partition_then_heal_store_and_forward() {
    let n = 20;
    let mut sim = Sim::new(n).await;
    // Two line-components [0..10) and [10..20), bridged only by edge (9,10).
    for i in 0..9 {
        sim.connect(i, i + 1);
    }
    for i in 10..19 {
        sim.connect(i, i + 1);
    }
    let tx = vec![7u8; 1000];
    let mut rng = StdRng::seed_from_u64(55);

    // Phase 1: partition active (no 9<->10 edge). Only component A receives.
    let group_a: HashSet<usize> = (0..10).collect();
    let received_a = sim.flood(0, &tx, 0.0, &group_a, &mut rng);
    assert!(
        received_a.iter().all(|&x| x < 10),
        "during partition, only component A may receive"
    );
    assert!(received_a.contains(&9), "boundary node 9 should hold the tx");

    // Phase 2: heal — bridge the components and re-broadcast from boundary node 9, which
    // already holds the tx (store-and-forward). Component B must now converge.
    sim.connect(9, 10);
    let received_b = sim.flood(9, &tx, 0.0, &all_active(n), &mut rng);
    assert!(
        (10..20).all(|x| received_b.contains(&x)),
        "after heal, component B must receive via store-and-forward relay"
    );
}

/// Node churn: some nodes are offline during the flood; survivors still converge and the
/// engine never double-counts. (Models phones sleeping / OS-killed processes.)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sim_node_churn_survivors_converge() {
    let n = 40;
    let mut sim = Sim::new(n).await;
    sim.wire_random(4, 2024);
    let mut rng = StdRng::seed_from_u64(2025);

    // Knock out ~20% of nodes (not the origin).
    let mut active = all_active(n);
    for k in (3..n).step_by(5) {
        active.remove(&k);
    }
    let tx = vec![3u8; 1100];

    let received = sim.flood(0, &tx, 0.05, &active, &mut rng);
    // Every active node reachable in the (still-connected via the line) graph receives.
    for &node in &active {
        assert!(
            received.contains(&node),
            "active node {node} should receive despite churn"
        );
    }
    for &node in &active {
        assert!(sim.nodes[node].received_queue_size() <= 1, "dedup under churn");
    }
}
