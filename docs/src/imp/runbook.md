# IMP Pipeline Runbook

_Operational guide for the Interleaved Mastery Pipeline (IMP)._

## Key Tasks

```bash
# Generate capsule session for a student
curl -X POST http://localhost:5150/api/v1/management/capsule-sessions/generate \
  -H "Content-Type: application/json" \
  -d '{"student_id": "student_001", "anki2_path": "/data/collections/student_001.anki2"}'

# Check weekly progress
curl "http://localhost:5150/api/v1/management/compliance/weekly?session_week=2026-W28"

# Search students
curl "http://localhost:5150/api/v1/management/compliance/students?q=student"

# Chat with AI
curl -X POST http://localhost:5150/api/v1/management/ask \
  -H "Content-Type: application/json" \
  -d '{"query": "Which students are struggling?"}'
```

## Management Console Access

- **Dashboard**: `http://localhost:5150/dashboard`
- **IMP Console**: `http://localhost:5150/imp-console`
  - Sidebar toggle (`◀`) collapses the sidebar for more content space
  - Progressive search in Student Progress page
  - AI Chatbox for natural language queries