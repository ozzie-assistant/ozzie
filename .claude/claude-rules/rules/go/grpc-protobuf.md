---
title: "Go gRPC & Protobuf"
---

## Proto source and generated code

- Proto files (`.proto`) live in their own package (e.g. `proto/`).
- Generated files (`*.pb.go`, `*_grpc.pb.go`) are **committed to the repository** so that `go build` works without `protoc` installed.
- Place a `generate.go` in the proto package with a `//go:generate` directive for regeneration:

```go
package proto

//go:generate protoc --go_out=.. --go_opt=module=<module> --go-grpc_out=.. --go-grpc_opt=module=<module> <file>.proto
```

- `go generate` is a **dev-time tool**, not a CI step. CI builds against the committed `.pb.go` files.
- Optional CI check: regenerate + `git diff --exit-code` to verify `.pb.go` files are up to date.

## Never edit generated code

- Do not modify `*.pb.go` or `*_grpc.pb.go` by hand. Change the `.proto` source and regenerate.
- Do not add custom methods to generated types. Wrap them in domain types instead.

## Hexagonal boundaries

- Proto-generated code is **infrastructure** — it belongs outside `internal/core/`.
- Core domain types must not depend on protobuf types. Map between proto messages and domain types at the adapter boundary.
- The gRPC server implementation lives in `internal/infra/grpc/` (or similar infra package).
- Core handlers receive plain domain structs, never `pb.*` messages.

## Proto design

- Keep messages **payload-agnostic** where possible: use `string` or `bytes` for opaque payloads that evolve independently of the proto contract.
- Use `oneof` for polymorphic updates (e.g. event vs result in a stream).
- Avoid importing `google/protobuf/empty.proto` for trivial ack responses — define a local `message Ack {}` to keep the proto self-contained.
