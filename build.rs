// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The Helm chart is compiled into the binary (src/cli/chart.rs); rebuild
    // when it changes.
    println!("cargo:rerun-if-changed=chart");

    #[cfg(feature = "grpc")]
    {
        tonic_prost_build::compile_protos("proto/flow.proto")?;
    }
    Ok(())
}
