//! Hivemind - Distributed Rate Limiting Service
//!
//! This crate implements a distributed rate limiting service that integrates
//! with Envoy Proxy's global rate limiting API. It uses a peer-to-peer mesh
//! architecture for state synchronization without relying on centralized storage.

pub mod grpc;
pub mod ratelimit;
pub mod config;
pub mod error;
pub mod mesh;
