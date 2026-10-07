# Compliance Tracking

Tracks how many study sessions each student completes per week vs. their assigned N-value target.

## Metrics

| Metric | Formula | Description |
|---|---|---|
| Compliance % | `sessions_completed / sessions_target` | Weekly progress rate |
| Avg Cards/Session | `total_cards / sessions_completed` | Engagement depth |
| Below Threshold | `compliance < 0.70` | Students needing attention |

## API Endpoints

### Weekly Progress Report

```http
GET /api/v1/management/compliance/weekly?session_week=2026-W28&page=1&per_page=50&below_threshold=0.7
```

### Student History

```http
GET /api/v1/management/compliance/student/student_001
```

Returns per-week compliance records for the student, sorted by week descending.

### Student Search (Progressive)

```http
GET /api/v1/management/compliance/students?q=student
```

Returns matching student IDs for type-ahead search in the IMP Console.

## Management Console

The IMP Console at `http://localhost:5150/imp-console` provides:
- **Study Session** — Generate new sessions for any student
- **Weekly Progress** — View class-wide progress with color-coded compliance
- **Student Progress** — Per-student timeline with progress bars
- **AI Chatbox** — Ask natural language questions about student data