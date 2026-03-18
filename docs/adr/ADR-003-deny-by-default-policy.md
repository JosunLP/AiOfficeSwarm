# ADR-003: Deny-by-Default Policy Engine

**Status**: Accepted  
**Date**: 2026-03-16

## Context

The framework needs a policy evaluation system for admission control. Two
common defaults exist: allow-by-default (permissive) and deny-by-default
(restrictive).

## Decision

The **production-recommended posture is deny-by-default**. `PolicyEngine::deny_by_default()`
is the intended default for production deployments. `allow_by_default()` is
provided for development and testing.

## Rationale

- Fail-secure: new deployments are restricted until policies are explicitly configured.
- Reduces the blast radius of misconfiguration.
- Aligns with the principle of least privilege.

## Consequences

- Operators must explicitly add `AllowAllPolicy` or custom policies to permit
  actions in a deny-by-default engine.
- Development setups should use `allow_by_default()` to avoid friction.
