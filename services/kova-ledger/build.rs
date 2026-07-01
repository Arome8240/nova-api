fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true)
        .build_client(false)
        .compile_protos(
            &["../../proto/kova_ledger.proto"],
            // ../../proto  — local proto root
            // /usr/include — google/protobuf well-known types (protoc 3.x, Ubuntu)
            &["../../proto", "/usr/include"],
        )?;
    Ok(())
}
