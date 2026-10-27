//! 应用的 gRPC Protobuf 类型。

pub mod echo {
    tonic::include_proto!("example.v1");
}
