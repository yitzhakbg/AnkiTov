# Anki Operations Access

_Operational guide for Anki operations via the Management Console._

Anki operations are performed through the Management Console's Anki Operations panel, backed by the backend's AnkiConnect integration (`backend/src/services/anki_connect.rs`). Destructive operations (card creation, note editing) are deactivated.

## Available Operations

- Health check (`/api/v1/management/anki-ops/health`)
- Sync collection
- Find problems
- Retention stats
- Deck health reports
- Backup collection

All operations are logged to the immutable audit ledger.