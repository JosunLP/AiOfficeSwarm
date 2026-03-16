# ADR-005: DashMap for Concurrent Registry State

**Status**: Accepted  
**Date**: 2026-03-16

## Context

The agent registry and plugin registry are read-heavy (the scheduler reads agent
records on every dispatch) and occasionally written (registration, status updates).
A `Mutex<HashMap>` would serialize all reads.

## Decision

Use `DashMap` (sharded concurrent hashmap) for the registry's internal storage.
Each `DashMap` shard has its own `RwLock`, enabling truly concurrent reads.

## Rationale

- Concurrent reads without writer starvation.
- No global lock contention under typical scheduler load.
- Simple API identical to `HashMap`.

## Consequences

- `DashMap v5` is a stable, widely-used crate.
- Slightly higher memory overhead than `HashMap` due to sharding.
- Iteration order is not guaranteed (acceptable for the registry use case).
