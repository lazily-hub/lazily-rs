# Durable NATS JetStream transport

The `durable-jetstream` feature is a transport adapter around the PostgreSQL
durable owner. NATS carries leased deliveries and publications; it never decides
whether domain state exists or has advanced.

## Ordering contract

Ingress follows one order:

1. pull one typed `JetStreamEnvelope` from a durable consumer;
2. commit inbox identity, owner state, projection, timers, and outbox effects in
   one PostgreSQL transaction;
3. send an explicit ACK and wait for the server's confirmation.

If the commit fails, no ACK is sent. If the process stops after commit but before
ACK, JetStream redelivers and PostgreSQL inbox deduplication returns the original
durable outcome before the delivery is acknowledged. The raw broker message is
private to `JetStreamDelivery`, so the public API does not expose a terminal ACK
that can bypass this sequence.

`progress()` sends only `+WPI`, extending `AckWait` without changing durable
state. `retry()` sends NAK. `MaxAckPending` supplies the consumer-side in-flight
bound, and `begin_drain()` prevents new pulls while leaving already-returned
deliveries usable.

Malformed or unsupported envelopes are poison. `persist_poison_then_term`
stores their transport delivery identity, diagnostic, and original bytes in
PostgreSQL, publishes a dead-letter record with a stable message ID, and only
then sends TERM. Any earlier failure leaves the broker delivery eligible for
redelivery.

## Publication and receipt recovery

The relay derives `Nats-Msg-Id` from the durable owner and outbox effect
identities. It publishes first and stores the PostgreSQL receipt second. A crash
between those operations causes the outbox lease to expire; retrying the same
identity is recognized by JetStream's duplicate window, after which the receipt
retires the effect. A publish failure stores no receipt.

The wakeup source is intentionally disposable. A notification may prompt a
worker to query PostgreSQL, but it is never evidence that durable work exists.

## Runtime and verification

The adapter uses `async-nats` 0.50 under its Apache-2.0 license. Its async entry
points run on Tokio's multi-thread runtime because the existing PostgreSQL host
is synchronous; the adapter isolates its own receipt/disposition calls with
`block_in_place`, and application commit callbacks should do the same or move
their database operation to a dedicated blocking executor.

Run:

```sh
./scripts/test-durable-jetstream.sh
```

The script starts isolated real PostgreSQL and NATS 2.12.1 JetStream services
when URLs are not supplied. The corpus covers failed commit/no ACK, commit before
ACK process loss, confirmed ACK, progress and NAK behavior, bounded pending
deliveries, drain, durable poison disposition, publish failure, publish before
receipt process loss, duplicate publication, and final receipt retirement.

The protocol choices follow the [NATS consumer acknowledgement and redelivery
contract](https://docs.nats.io/nats-concepts/jetstream/consumers), the official
[`async-nats` pull-consumer example](https://github.com/nats-io/nats.rs/blob/main/async-nats/examples/jetstream_pull.rs),
and the [`async-nats` JetStream message API](https://docs.rs/async-nats/0.50.0/async_nats/jetstream/message/index.html).
