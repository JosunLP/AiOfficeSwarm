# ADR-001: Use Rust as the Implementation Language

**Status**: Accepted  
**Date**: 2026-03-16

## Context

The framework must be performant, safe, and suitable for enterprise deployments.
It must run without a managed runtime overhead and support async I/O natively.

## Decision

Use **Rust** as the sole implementation language.

## Consequences

- Memory safety without garbage collection.
- First-class async support via Tokio.
- Strong type system enforces domain contracts at compile time.
- `#![forbid(unsafe_code)]` ensures memory safety throughout.
- Learning curve for contributors unfamiliar with Rust.
