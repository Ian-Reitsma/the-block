#![allow(dead_code)]

#[path = "../../tools/log_indexer.rs"]
mod log_indexer_impl;

pub use log_indexer_impl::{
    index_logs, index_logs_with_options, search_logs, IndexOptions, LogEntry, LogFilter,
    LogIndexerError, Result as LogIndexerResult,
};
