//! gRPC server implementation.

use std::net::SocketAddr;
use std::sync::Arc;
use tonic::transport::Server;
use tracing::{info, error};

use super::proto::envoy::service::ratelimit::v3::rate_limit_service_server::RateLimitServiceServer;
use super::service::RateLimitServiceImpl;
use crate::error::{HivemindError, Result};
use crate::ratelimit::{RateLimiter, RateLimiterBackend, DistributedRateLimiter};

/// gRPC server for the rate limit service.
pub struct GrpcServer<R: RateLimiterBackend + 'static> {
    /// Address to bind to
    addr: SocketAddr,
    /// The rate limiter instance
    rate_limiter: Arc<R>,
}

impl GrpcServer<RateLimiter> {
    /// Create a new gRPC server with a local rate limiter.
    pub fn new(addr: SocketAddr, rate_limiter: Arc<RateLimiter>) -> Self {
        Self { addr, rate_limiter }
    }
}

impl GrpcServer<DistributedRateLimiter> {
    /// Create a new gRPC server with a distributed rate limiter.
    pub fn with_distributed_limiter(addr: SocketAddr, rate_limiter: Arc<DistributedRateLimiter>) -> Self {
        Self { addr, rate_limiter }
    }
}

impl<R: RateLimiterBackend + 'static> GrpcServer<R> {
    /// Start the gRPC server.
    ///
    /// This method will block until the server is shut down.
    pub async fn serve(self) -> Result<()> {
        let service = RateLimitServiceImpl::new(self.rate_limiter);

        info!(
            addr = %self.addr,
            "Starting gRPC server for RateLimitService"
        );

        Server::builder()
            .add_service(RateLimitServiceServer::new(service))
            .serve(self.addr)
            .await
            .map_err(|e| {
                error!(error = %e, "gRPC server failed");
                HivemindError::Grpc(e)
            })
    }

    /// Start the gRPC server with graceful shutdown.
    ///
    /// The server will shut down when the provided signal resolves.
    pub async fn serve_with_shutdown<F>(self, signal: F) -> Result<()>
    where
        F: std::future::Future<Output = ()> + Send,
    {
        let service = RateLimitServiceImpl::new(self.rate_limiter);

        info!(
            addr = %self.addr,
            "Starting gRPC server for RateLimitService with graceful shutdown"
        );

        Server::builder()
            .add_service(RateLimitServiceServer::new(service))
            .serve_with_shutdown(self.addr, signal)
            .await
            .map_err(|e| {
                error!(error = %e, "gRPC server failed");
                HivemindError::Grpc(e)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_creation() {
        let addr: SocketAddr = "127.0.0.1:8081".parse().unwrap();
        let rate_limiter = Arc::new(RateLimiter::new());
        let _server = GrpcServer::new(addr, rate_limiter);
    }
}
