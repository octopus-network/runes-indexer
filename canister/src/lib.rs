mod bitcoin_api;
pub mod config;
pub mod etchin_tasks;
pub mod index;
mod into_usize;
mod notifier;
pub mod rpc;
use anyhow::Error;
use chrono::{DateTime, TimeZone, Utc};

type Result<T = (), E = Error> = std::result::Result<T, E>;

fn timestamp(seconds: u64) -> DateTime<Utc> {
  Utc
    .timestamp_opt(seconds.try_into().unwrap_or(i64::MAX), 0)
    .unwrap()
}
