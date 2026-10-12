fn main() {
    let protos = [
        "src/proto_src/device_to_device_messages.proto",
        "src/proto_src/offline_wire_formats.proto",
        "src/proto_src/securegcm.proto",
        "src/proto_src/securemessage.proto",
        "src/proto_src/ukey.proto",
        "src/proto_src/wire_format.proto",
    ];
    for path in protos {
        println!("cargo:rerun-if-changed={path}");
    }
    prost_build::compile_protos(&protos, &["src/proto_src"])
        .expect("Google Nearby protobuf generation failed");
}
