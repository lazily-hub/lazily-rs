# Durable runtime implementation provenance

This note covers the independently authored Postgres durable-owner host and the
JetStream transport that delivers work to that authority. It is an engineering
provenance record, not legal advice or a substitute for contributor agreements.

Implementation source digest: `sha256:0b44348e20d905d81503f1c514b966eb8041ee89cf85c0a039d4c66f3f05af30`

## Official public sources

The implementation uses public protocol and product documentation as conceptual
references, without copying source code, diagrams, fixtures, examples, or prose:

- [PostgreSQL 18 documentation](https://www.postgresql.org/docs/18/) for
  transaction, locking, isolation, and durability semantics.
- [NATS JetStream documentation](https://docs.nats.io/nats-concepts/jetstream)
  for delivery, acknowledgement, redelivery, and consumer semantics.
- [rust-postgres](https://github.com/sfackler/rust-postgres) as the official
  upstream source for the selected synchronous Postgres client family.
- [nats.rs](https://github.com/nats-io/nats.rs) as the official upstream source
  for the selected Rust NATS client.

## Independent authorship

The Lazily APIs, migrations, transaction boundaries, state transitions, tests,
documentation, and failure handling covered by this note were independently authored
for this repository from its public specifications and the sources above. No
third-party implementation expression was used as transformation input.

## Selected dependencies

- `postgres 0.19.14` is the synchronous database client used to keep the
  authoritative transaction boundary explicit. Its exact enabled features and
  transitive graph are recorded in `dependency-license-inventory.json`.
- `async-nats 0.50.0` is the JetStream client. Broker delivery remains outside
  the Postgres authority boundary. Its exact enabled features and transitive
  graph are recorded in the same inventory.

## Synthetic fixtures

Durable-runtime tests and fixtures use synthetic identifiers, subjects, payloads,
workflow names, timings, and failure cases created for Lazily. They do not derive
from production records or private product schemas.

## Excluded inputs

No employer-owned or customer-specific code, schemas, identifiers, events,
policies, incidents, fixtures, credentials, logs, or confidential materials were
used to design or test this implementation. Work on this public implementation
is separate from any private employer boundary or attestation.
