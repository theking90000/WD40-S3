fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(any(feature = "grpc-client", feature = "grpc-server"))]
    tonic_prost_build::configure()
        .btree_map(".")
        .build_client(cfg!(feature = "grpc-client"))
        .build_server(cfg!(feature = "grpc-server"))
        .compile_protos(&["proto/registry.proto"], &["proto"])?;
    Ok(())
}
