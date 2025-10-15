#![allow(dead_code)]

#[path = "../../tools/log_indexer.rs"]
mod log_indexer_impl;

pub use log_index::{IndexOptions, LogEntry, LogFilter};
pub use log_indexer_impl::{
    index_logs, index_logs_with_options, search_logs, LogIndexerError, Result as LogIndexerResult,
};
