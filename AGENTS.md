# Tessera Codex Instructions

Tessera is a Rust project for building a high-performance, production-ready DSL
for implementing CUDA kernels.

The DSL should let kernel authors express the mathematical meaning of tensor
operations separately from the underlying memory layout, access pattern, and
implementation strategy. The compiler and standard library should preserve full
control over layouts and representation so kernels can match hand-written CUDA
implementations used in production systems such as vLLM and SGLang.

## Project Goals

- Build a DSL named **Tessera** for authoring CUDA kernels.
- Make tensor operations readable at the mathematical/operator level.
- Keep memory layout, tiling, swizzling, paging, vectorization, and access
  patterns explicit and controllable.
- Support rewriting production hand-written kernels without changing their
  algorithm, memory format, synchronization strategy, or performance-critical
  access pattern.
- Implement the compiler, runtime support, and tooling in Rust.

## Core Design Direction

- Separate tensor semantics from representation details.
- Treat layouts as first-class, explicit concepts rather than hidden compiler
  guesses.
- Prefer abstractions that compile away into predictable pointer arithmetic,
  CUDA memory operations, synchronization, and target intrinsics.
- Do not introduce a high-level abstraction unless it can preserve or expose the
  low-level performance contract needed by production kernels.
- Keep the design compatible with kernels that use paged KV caches, block
  tables, custom strides, shared-memory swizzles, cooperative groups, async
  copies, and tensor-core instructions.

## Rust Implementation Guidance

- Use idiomatic Rust with clear module boundaries and explicit data models.
- Prefer typed IR/data structures over stringly typed representations.
- Keep compiler phases explicit: parse, resolve, type/check, lower, optimize,
  verify, and emit.
- Make invariants visible in types when practical.
- Add comments only where they clarify non-obvious compiler or kernel semantics.
- Avoid premature framework choices; keep early code small, testable, and easy to
  refactor.

## Engineering Standards

- Preserve the distinction between source-level DSL concepts and lowered CUDA
  implementation details.
- Prefer deterministic lowering over heuristic optimization for core kernel
  semantics.
- When modeling production kernels, document the memory layout, access pattern,
  synchronization requirements, and expected lowered form.
- Add tests for parsing, type checking, lowering, and layout/indexing behavior as
  soon as the corresponding infrastructure exists.
- Do not silently relax correctness or performance assumptions. If a lowering
  requires alignment, contiguity, uniform control, bounds, or layout facts, model
  and check those facts explicitly.

## Working With This Repo

- Check `git status --short --branch` before editing.
- Use `rg` for searching files and text.
- Keep generated artifacts, build output, dependency directories, and local
  secrets out of Git.
- Use focused commits with direct messages describing the design or code change.
- If implementation choices diverge from documented design intent, update the
  relevant documentation in the same change.

## Current State

The repository is intentionally minimal. Establish foundational documents and
Rust project structure before adding compiler implementation code.
