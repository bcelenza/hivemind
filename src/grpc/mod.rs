//! gRPC server module for Envoy rate limit service.

mod server;
mod service;

pub use server::GrpcServer;
pub use service::RateLimitServiceImpl;

// Include the generated protobuf code
pub mod proto {
    pub mod envoy {
        pub mod extensions {
            pub mod common {
                pub mod ratelimit {
                    pub mod v3 {
                        tonic::include_proto!("envoy.extensions.common.ratelimit.v3");
                    }
                }
            }
        }
        pub mod service {
            pub mod ratelimit {
                pub mod v3 {
                    tonic::include_proto!("envoy.service.ratelimit.v3");
                }
            }
        }
    }
}

// Re-export commonly used types
pub use proto::envoy::service::ratelimit::v3::{
    rate_limit_service_server::RateLimitServiceServer,
    RateLimitRequest, RateLimitResponse,
};
pub use proto::envoy::extensions::common::ratelimit::v3::RateLimitDescriptor;
