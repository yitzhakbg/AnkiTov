# Capsule (Study Session) Generation

Capsules are generated server-side by the Management Console backend and tracked per-student.

## Generation Algorithm

1. **Query overdue cards** from the student's assigned track profile (libSQL)
2. **Sort by FSRS urgency** (lowest retention probability first)
3. **Guarantee minimum per-track presence** in the interleaved mix
4. **Slice** the card pool into N equal chunks (N = N-Lever value)
5. **Apply hard cap** (max cards per session)
6. **Spill-over** remaining cards to the next session

## API Endpoint

```http
POST /api/v1/management/capsule-sessions/generate
Content-Type: application/json

{
  "student_id": "student_001",
  "anki2_path": "/data/collections/student_001.anki2",
  "session_week": "2026-W28"
}
```

Response:
```json
{
  "session_id": "uuid-string",
  "card_ids": [1, 2, 3, ...],
  "capsule_size": 25,
  "n_value": 3,
  "session_duration_minutes": 20,
  "status": "pending"
}
```

## Capsule Session Entity

```rust
// @see backend/src/models/entities/capsule_session.rs
pub struct CapsuleSession {
    pub id: String,
    pub student_id: String,
    pub track_profile_id: String,
    pub n_value: i32,
    pub session_duration_minutes: i32,
    pub card_ids: String,       // JSON array
    pub status: String,         // pending | in_progress | completed | expired
    pub session_week: String,   // ISO week "2026-W28"
    pub cards_completed: i32,
    pub cards_total: i32,
}
```