// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Compiles the vendored Cilium/Hubble protos (see proto/cilium/README.md).
// `protox` is a pure-Rust protobuf compiler, so no `protoc` is needed.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = "proto/cilium";
    let files = [
        "observer/observer.proto",
        "flow/flow.proto",
        "relay/relay.proto",
    ];
    println!("cargo:rerun-if-changed={root}");
    println!("cargo:rerun-if-changed=build.rs");

    let fds = protox::compile(files, [root])?;
    tonic_prost_build::configure()
        .build_server(true) // used by the in-process fake Observer in tests
        .build_client(true)
        .compile_fds(fds)?;
    Ok(())
}
