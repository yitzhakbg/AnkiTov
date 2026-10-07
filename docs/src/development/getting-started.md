# Getting Started

## Prerequisites

- Rust 2021 edition (`rustup`)
- Cargo
- Jujutsu (`brew install jj`)
- SQLite (bundled via `rusqlite`)

## Setup

```bash
# Clone the repo
git clone https://github.com/yitzhakbg/AnkiTov-C.git
cd AnkiTov

# Check the backend
cd backend && cargo check

# Run tests
cargo nextest run

# Start the development server
cargo run start

# The server starts on http://localhost:5150
```

## Key Commands

| Action | Command |
|---|---|
| Check compilation | `cd backend && cargo check` |
| Run tests | `cd backend && cargo nextest run` |
| Run dev server | `cd backend && cargo run start` |
| Generate rustdocs | `cd backend && cargo doc --no-deps --open` |
| View dashboard | `open http://localhost:5150/dashboard` |
| IMP Console | `open http://localhost:5150/imp-console` |