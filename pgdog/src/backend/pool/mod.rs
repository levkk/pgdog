//! Manage connections to the servers.

pub mod address;
pub mod ban;
pub mod cleanup;
pub mod cluster;
pub mod config;
pub mod connection;
pub mod error;
pub mod guard;
pub mod healthcheck;
pub mod inner;
pub mod monitor;
pub mod pool_impl;
pub mod replicas;
pub mod shard;
pub mod stats;

pub use address::Address;
pub use cluster::{Cluster, PoolConfig};
pub use config::Config;
pub use connection::Connection;
pub use error::Error;
pub use guard::Guard;
pub use healthcheck::Healtcheck;
use monitor::Monitor;
pub use pool_impl::Pool;
pub use replicas::Replicas;
pub use shard::Shard;

use ban::Ban;
use inner::Inner;
use pool_impl::Mapping;

#[cfg(test)]
mod test;
