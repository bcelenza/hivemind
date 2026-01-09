//! Peer Mesh networking module using Chitchat.
//!
//! This module implements the peer-to-peer mesh network used for
//! distributed state synchronization between Hivemind nodes.
//! It uses the chitchat library for gossip-based cluster membership
//! and state dissemination.

mod cluster;

pub use cluster::{Cluster, ClusterConfig, CounterKey};
