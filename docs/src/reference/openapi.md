# OpenAPI Specification

The backend generates an OpenAPI 3.0 spec automatically from `#[utoipa::path]` annotations on every controller route. The annotations are aggregated in `backend/src/openapi.rs` and **served live by the backend** — the spec is regenerated from the compiled route table on every request, so it can never drift from the running server.

## Live Endpoint

```http
GET http://localhost:5150/api/v1/openapi.json
```

## Interactive UI

The backend serves a ready-made [Scalar](https://scalar.com) reference at:

```http
GET http://localhost:5150/scalar
```

The page embeds the live spec above (Scalar itself loads from CDN).

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