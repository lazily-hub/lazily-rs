//! Phase 7 of the realtime + distributed primitives plan — `#lzservice`
//! embedded-service plane.
//!
//! See `lazily-spec/docs/service.md` and the formal model
//! `lazily-formal/LazilyFormal/Service.lean`. The story for "an instance is also
//! a host of services": `HealthCell` / `ReadinessCell` / `DiscoveryCell` /
//! `ServiceRegistry`, each a pure compute **core** (an aggregation / keyed map)
//! split from a reactive **cell** projecting the composed view.

use std::cell::RefCell;
use std::collections::BTreeMap;

use crate::Context;
use crate::cell::CellHandle;

// ===========================================================================
// Health
// ===========================================================================

/// Composed health status (worst component dominates).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Healthy,
    Degraded,
    Unhealthy,
}

/// Composed liveness-probe core. Each probe reports `up` and whether it is
/// `critical`.
#[derive(Debug, Clone, Default)]
pub struct HealthCore {
    probes: BTreeMap<String, (bool, bool)>, // name -> (up, critical)
}

impl HealthCore {
    pub fn new() -> Self {
        Self::default()
    }
    /// Set/refresh a probe.
    pub fn set(&mut self, name: impl Into<String>, up: bool, critical: bool) {
        self.probes.insert(name.into(), (up, critical));
    }
    /// The aggregate: Unhealthy if any critical probe is down, else Degraded if
    /// any is down, else Healthy.
    pub fn health(&self) -> Health {
        if self.probes.values().any(|(up, critical)| *critical && !*up) {
            Health::Unhealthy
        } else if self.probes.values().any(|(up, _)| !*up) {
            Health::Degraded
        } else {
            Health::Healthy
        }
    }
}

/// Reactive health: projects the aggregate onto a `Cell` for `/health`.
pub struct HealthCell {
    core: RefCell<HealthCore>,
    health: CellHandle<Health>,
}

impl HealthCell {
    pub fn new(ctx: &Context) -> Self {
        Self {
            core: RefCell::new(HealthCore::new()),
            health: ctx.cell(Health::Healthy),
        }
    }
    fn refresh(&self, ctx: &Context) {
        let h = self.core.borrow().health();
        self.health.set(ctx, h);
    }
    pub fn set(&self, ctx: &Context, name: impl Into<String>, up: bool, critical: bool) {
        self.core.borrow_mut().set(name, up, critical);
        self.refresh(ctx);
    }
    pub fn health(&self) -> Health {
        self.core.borrow().health()
    }
    pub fn health_cell(&self) -> CellHandle<Health> {
        self.health
    }
}

// ===========================================================================
// Readiness
// ===========================================================================

/// Composed readiness-probe core: ready iff every condition holds.
#[derive(Debug, Clone, Default)]
pub struct ReadinessCore {
    conditions: BTreeMap<String, bool>,
}

impl ReadinessCore {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn set(&mut self, name: impl Into<String>, ready: bool) {
        self.conditions.insert(name.into(), ready);
    }
    pub fn ready(&self) -> bool {
        self.conditions.values().all(|r| *r)
    }
}

/// Reactive readiness: projects `ready` onto a `Cell` for `/ready`.
pub struct ReadinessCell {
    core: RefCell<ReadinessCore>,
    ready: CellHandle<bool>,
}

impl ReadinessCell {
    pub fn new(ctx: &Context) -> Self {
        Self {
            core: RefCell::new(ReadinessCore::new()),
            ready: ctx.cell(true),
        }
    }
    fn refresh(&self, ctx: &Context) {
        let r = self.core.borrow().ready();
        self.ready.set(ctx, r);
    }
    pub fn set(&self, ctx: &Context, name: impl Into<String>, ready: bool) {
        self.core.borrow_mut().set(name, ready);
        self.refresh(ctx);
    }
    pub fn ready(&self) -> bool {
        self.core.borrow().ready()
    }
    pub fn ready_cell(&self) -> CellHandle<bool> {
        self.ready
    }
}

// ===========================================================================
// Discovery
// ===========================================================================

/// Service-discovery core: `service → (endpoint, owner)`. A peer's departure
/// (`evict`) removes its endpoints.
#[derive(Debug, Clone, Default)]
pub struct DiscoveryCore<P> {
    entries: BTreeMap<String, (String, P)>,
}

impl<P: Clone + PartialEq> DiscoveryCore<P> {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }
    pub fn register(&mut self, service: impl Into<String>, endpoint: impl Into<String>, peer: P) {
        self.entries.insert(service.into(), (endpoint.into(), peer));
    }
    pub fn deregister(&mut self, service: &str) {
        self.entries.remove(service);
    }
    /// Remove all endpoints owned by `peer` (membership loss).
    pub fn evict(&mut self, peer: &P) {
        self.entries.retain(|_, (_, owner)| owner != peer);
    }
    pub fn resolve(&self, service: &str) -> Option<String> {
        self.entries.get(service).map(|(e, _)| e.clone())
    }
    /// The live `service → endpoint` map.
    pub fn discovery(&self) -> BTreeMap<String, String> {
        self.entries
            .iter()
            .map(|(s, (e, _))| (s.clone(), e.clone()))
            .collect()
    }
}

/// Reactive service discovery.
pub struct DiscoveryCell<P> {
    core: RefCell<DiscoveryCore<P>>,
    discovery: CellHandle<BTreeMap<String, String>>,
}

impl<P: Clone + PartialEq + 'static> DiscoveryCell<P> {
    pub fn new(ctx: &Context) -> Self {
        Self {
            core: RefCell::new(DiscoveryCore::new()),
            discovery: ctx.cell(BTreeMap::new()),
        }
    }
    fn refresh(&self, ctx: &Context) {
        let d = self.core.borrow().discovery();
        self.discovery.set(ctx, d);
    }
    pub fn register(
        &self,
        ctx: &Context,
        service: impl Into<String>,
        endpoint: impl Into<String>,
        peer: P,
    ) {
        self.core.borrow_mut().register(service, endpoint, peer);
        self.refresh(ctx);
    }
    pub fn deregister(&self, ctx: &Context, service: &str) {
        self.core.borrow_mut().deregister(service);
        self.refresh(ctx);
    }
    pub fn evict(&self, ctx: &Context, peer: &P) {
        self.core.borrow_mut().evict(peer);
        self.refresh(ctx);
    }
    pub fn resolve(&self, service: &str) -> Option<String> {
        self.core.borrow().resolve(service)
    }
    pub fn discovery(&self, ctx: &Context) -> BTreeMap<String, String> {
        self.discovery.get(ctx)
    }
    pub fn discovery_cell(&self) -> CellHandle<BTreeMap<String, String>> {
        self.discovery
    }
}

// ===========================================================================
// Service registry (durable)
// ===========================================================================

/// A durable registry op (the ordered log entry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryOp {
    Register { service: String, endpoint: String },
    Deregister { service: String },
}

/// Durable service-registry core: an ordered log (the `DurableOutbox` pattern)
/// whose left-fold is the projection, so replay reconstructs it.
#[derive(Debug, Clone, Default)]
pub struct ServiceRegistryCore {
    log: Vec<RegistryOp>,
    projection: BTreeMap<String, String>,
}

impl ServiceRegistryCore {
    pub fn new() -> Self {
        Self::default()
    }
    fn apply(projection: &mut BTreeMap<String, String>, op: &RegistryOp) {
        match op {
            RegistryOp::Register { service, endpoint } => {
                projection.insert(service.clone(), endpoint.clone());
            }
            RegistryOp::Deregister { service } => {
                projection.remove(service);
            }
        }
    }
    pub fn register(&mut self, service: impl Into<String>, endpoint: impl Into<String>) {
        let op = RegistryOp::Register {
            service: service.into(),
            endpoint: endpoint.into(),
        };
        Self::apply(&mut self.projection, &op);
        self.log.push(op);
    }
    pub fn deregister(&mut self, service: impl Into<String>) {
        let op = RegistryOp::Deregister {
            service: service.into(),
        };
        Self::apply(&mut self.projection, &op);
        self.log.push(op);
    }
    /// Rebuild the projection from the durable log (restart / crash-replay).
    pub fn replay(&mut self) {
        let mut projection = BTreeMap::new();
        for op in &self.log {
            Self::apply(&mut projection, op);
        }
        self.projection = projection;
    }
    pub fn projection(&self) -> BTreeMap<String, String> {
        self.projection.clone()
    }
    pub fn log(&self) -> &[RegistryOp] {
        &self.log
    }
}

/// Reactive durable service registry.
pub struct ServiceRegistry {
    core: RefCell<ServiceRegistryCore>,
    projection: CellHandle<BTreeMap<String, String>>,
}

impl ServiceRegistry {
    pub fn new(ctx: &Context) -> Self {
        Self {
            core: RefCell::new(ServiceRegistryCore::new()),
            projection: ctx.cell(BTreeMap::new()),
        }
    }
    fn refresh(&self, ctx: &Context) {
        let p = self.core.borrow().projection();
        self.projection.set(ctx, p);
    }
    pub fn register(&self, ctx: &Context, service: impl Into<String>, endpoint: impl Into<String>) {
        self.core.borrow_mut().register(service, endpoint);
        self.refresh(ctx);
    }
    pub fn deregister(&self, ctx: &Context, service: impl Into<String>) {
        self.core.borrow_mut().deregister(service);
        self.refresh(ctx);
    }
    pub fn replay(&self, ctx: &Context) {
        self.core.borrow_mut().replay();
        self.refresh(ctx);
    }
    pub fn projection(&self, ctx: &Context) -> BTreeMap<String, String> {
        self.projection.get(ctx)
    }
    pub fn projection_cell(&self) -> CellHandle<BTreeMap<String, String>> {
        self.projection
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_worst_component() {
        let mut h = HealthCore::new();
        h.set("cache", true, false);
        assert_eq!(h.health(), Health::Healthy);
        h.set("cache", false, false);
        assert_eq!(h.health(), Health::Degraded);
        h.set("db", false, true);
        assert_eq!(h.health(), Health::Unhealthy);
    }

    #[test]
    fn readiness_all_conditions() {
        let mut r = ReadinessCore::new();
        r.set("deps", false);
        assert!(!r.ready());
        r.set("deps", true);
        assert!(r.ready());
    }

    #[test]
    fn discovery_evict_removes() {
        let mut d = DiscoveryCore::<u64>::new();
        d.register("api", "e1", 1);
        d.register("db", "e2", 2);
        d.evict(&2);
        assert_eq!(d.discovery().len(), 1);
        assert_eq!(d.resolve("api"), Some("e1".to_string()));
    }

    #[test]
    fn registry_replay_reconstructs() {
        let mut reg = ServiceRegistryCore::new();
        reg.register("api", "v1");
        reg.register("api", "v2");
        reg.deregister("db");
        let before = reg.projection();
        reg.replay();
        assert_eq!(reg.projection(), before);
        assert_eq!(reg.projection().get("api"), Some(&"v2".to_string()));
    }
}
