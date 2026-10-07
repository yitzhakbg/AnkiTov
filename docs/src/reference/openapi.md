---
type: note
title: OpenAPI Specification
---

# OpenAPI Specification

The backend generates an OpenAPI 3.0 spec automatically from `#[utoipa::path]` annotations on every controller route.

## Live Endpoint

```http
GET http://localhost:5150/api/v1/openapi.json
```

## Use with Scalar

The spec is designed for [Scalar](https://scalar.com) — an interactive API playground. To view:

```bash
# Install Scalar CLI (or use the web version)
npx @scalar/cli serve http://localhost:5150/api/v1/openapi.json
```

Or use Swagger UI:

```bash
npx swagger-ui-cli http://localhost:5150/api/v1/openapi.json
```

## Annotation Convention

Every controller route includes:

```rust
#[utoipa::path(
    get,
    path = "/management/compliance/weekly",
    params(ComplianceQuery),
    responses(
        (status = 200, description = "Weekly compliance report")
    ),
    tag = "Management Console"
)]
pub async fn weekly_compliance(...) -> Result<Response> { ... }
```
