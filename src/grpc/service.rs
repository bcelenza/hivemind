//! Rate limit service implementation.

use std::sync::Arc;
use tonic::{Request, Response, Status};
use tracing::{debug, info, instrument, warn};

use super::proto::envoy::service::ratelimit::v3::{
    rate_limit_service_server::RateLimitService,
    rate_limit_response::Code,
    RateLimitRequest, RateLimitResponse,
};

use crate::ratelimit::RateLimiterBackend;

/// Implementation of the Envoy RateLimitService gRPC interface.
pub struct RateLimitServiceImpl<R: RateLimiterBackend> {
    /// The rate limiter instance
    rate_limiter: Arc<R>,
}

impl<R: RateLimiterBackend> RateLimitServiceImpl<R> {
    /// Create a new RateLimitServiceImpl with the given rate limiter.
    pub fn new(rate_limiter: Arc<R>) -> Self {
        Self { rate_limiter }
    }
}

#[tonic::async_trait]
impl<R: RateLimiterBackend + 'static> RateLimitService for RateLimitServiceImpl<R> {
    /// Determine whether rate limiting should take place.
    ///
    /// This method processes rate limit requests from Envoy proxies and returns
    /// a decision based on the current state of the rate limiter.
    #[instrument(
        skip(self, request),
        fields(
            domain = %request.get_ref().domain,
            descriptor_count = request.get_ref().descriptors.len(),
            hits_addend = request.get_ref().hits_addend
        )
    )]
    async fn should_rate_limit(
        &self,
        request: Request<RateLimitRequest>,
    ) -> Result<Response<RateLimitResponse>, Status> {
        let req = request.into_inner();

        debug!(
            domain = %req.domain,
            descriptors = ?req.descriptors,
            hits_addend = req.hits_addend,
            "Processing rate limit request"
        );

        // Validate the request
        if req.domain.is_empty() {
            warn!("Received rate limit request with empty domain");
            return Err(Status::invalid_argument("domain is required"));
        }

        if req.descriptors.is_empty() {
            warn!("Received rate limit request with no descriptors");
            return Err(Status::invalid_argument("at least one descriptor is required"));
        }

        // Get the number of hits to add (default to 1 if not specified)
        let hits = if req.hits_addend == 0 { 1 } else { req.hits_addend };

        // Check rate limits for each descriptor
        let mut statuses = Vec::with_capacity(req.descriptors.len());
        let mut overall_code = Code::Ok;

        for descriptor in &req.descriptors {
            let status = self.rate_limiter
                .check_rate_limit(&req.domain, descriptor, hits)
                .await;

            // If any descriptor is over limit, the overall response is over limit
            if status.code() == Code::OverLimit {
                overall_code = Code::OverLimit;
            }

            statuses.push(status);
        }

        let response = RateLimitResponse {
            overall_code: overall_code.into(),
            statuses,
            response_headers_to_add: Vec::new(),
            request_headers_to_add: Vec::new(),
            raw_body: Vec::new(),
            dynamic_metadata: None,
        };

        info!(
            domain = %req.domain,
            overall_code = ?overall_code,
            "Rate limit decision made"
        );

        Ok(Response::new(response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ratelimit::RateLimiter;
    use crate::grpc::proto::envoy::extensions::common::ratelimit::v3::{
        rate_limit_descriptor::Entry,
        RateLimitDescriptor,
    };

    #[tokio::test]
    async fn test_empty_domain_rejected() {
        let rate_limiter = Arc::new(RateLimiter::new());
        let service = RateLimitServiceImpl::new(rate_limiter);

        let request = Request::new(RateLimitRequest {
            domain: String::new(),
            descriptors: vec![RateLimitDescriptor {
                entries: vec![Entry {
                    key: "test".to_string(),
                    value: "value".to_string(),
                }],
                limit: None,
            }],
            hits_addend: 1,
        });

        let result = service.should_rate_limit(request).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn test_empty_descriptors_rejected() {
        let rate_limiter = Arc::new(RateLimiter::new());
        let service = RateLimitServiceImpl::new(rate_limiter);

        let request = Request::new(RateLimitRequest {
            domain: "test".to_string(),
            descriptors: vec![],
            hits_addend: 1,
        });

        let result = service.should_rate_limit(request).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn test_valid_request_returns_ok() {
        let rate_limiter = Arc::new(RateLimiter::new());
        let service = RateLimitServiceImpl::new(rate_limiter);

        let request = Request::new(RateLimitRequest {
            domain: "test".to_string(),
            descriptors: vec![RateLimitDescriptor {
                entries: vec![Entry {
                    key: "test_key".to_string(),
                    value: "test_value".to_string(),
                }],
                limit: None,
            }],
            hits_addend: 1,
        });

        let result = service.should_rate_limit(request).await;
        assert!(result.is_ok());

        let response = result.unwrap().into_inner();
        assert_eq!(response.overall_code, i32::from(Code::Ok));
        assert_eq!(response.statuses.len(), 1);
    }
}
