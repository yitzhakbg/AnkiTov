---
type: note
title: Rust Code Documentation
---

# Rust Code Documentation

The AnkiTov backend is thoroughly documented with Rust doc comments (`///` and `//!`).

## Generate and View

```bash
cd backend
cargo doc --no-deps --open
```

This opens `backend/target/doc/backend/index.html` in your browser.

## What's Documented

| Module | Coverage |
|---|---|
| `controllers` | All endpoints with `#[utoipa::path]` annotations |
| `models::entities` | All database entity models with field descriptions |
| `services` | Business logic services |
| `app` | Application hooks, routes, and configuration |

The doc includes auto-generated OpenAPI path specs that power the Scalar API reference.
