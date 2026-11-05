# lazily-rs v0.61.1 — SQLite store busy timeout

- `SqliteStore::open` sets an explicit 30 s SQLite busy timeout, exported as
  `SQLITE_STORE_BUSY_TIMEOUT`, so a second writer waits instead of failing with
  `database is locked`.
- Every store write is autocommit or `BEGIN IMMEDIATE`; a clippy guard bans
  deferred read-then-write transactions.
- Conforms to the lazily-spec v0.45.0 SQLite store contract
  (`sqlite-stores.json`).
