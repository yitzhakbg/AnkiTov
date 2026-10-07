# Track Library & Profiles

## Track Entity

```rust
// @see backend/src/models/entities/track.rs
pub struct Track {
    pub id: String,
    pub name: String,           // "7th Grade Math — Fractions"
    pub tags: String,           // JSON array of tag strings
    pub description: String,
    pub created_at: i64,
    pub updated_at: i64,
}
```

Tracks are tagged sub-collections. Tags enable cross-domain selection and FSRS parameter grouping.

## Track Profile Entity

```rust
// @see backend/src/models/entities/track_profile.rs
pub struct TrackProfile {
    pub id: String,
    pub name: String,           // "Math Remediation — Period 3"
    pub track_ids: String,      // JSON array of Track IDs
    pub target_retention: f64,  // FSRS desired retention (0.0–1.0)
    pub n_value: i32,           // Sessions per week
}
```

## API Endpoints

| Method | Route | Description |
|---|---|---|
| GET | `/api/v1/management/tracks` | List all tracks |
| POST | `/api/v1/management/tracks` | Create a track |
| GET | `/api/v1/management/track-profiles` | List all profiles |
| POST | `/api/v1/management/track-profiles` | Create a profile |
| POST | `/api/v1/management/track-profiles/:id/assign` | Assign to student(s) |