# Cilium / Hubble API definitions

These `.proto` files are copied unmodified from the Cilium project,
<https://github.com/cilium/cilium>, directory `api/v1`, at release **v1.20.2**:

- `observer/observer.proto`: the Hubble `Observer` gRPC service (`GetFlows`, `ServerStatus`, ...)
- `flow/flow.proto`: the `Flow` message and its enums
- `relay/relay.proto`: node status messages used by Hubble Relay

They are Copyright Authors of Hubble and licensed under the Apache License 2.0
(see the `SPDX-License-Identifier` header in each file and
<https://github.com/cilium/cilium/blob/main/LICENSE>).

`web-api/build.rs` compiles them with `protox`, a pure-Rust protobuf compiler, so
building needs no `protoc` binary. To update, replace the files with those from a
newer Cilium release and rebuild; Hubble's API is backward compatible.
