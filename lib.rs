// parng/lib.rs
//
// Copyright (c) 2016 Mozilla Foundation

//! A parallel PNG decoder.
//!
//! For the simple API, see the `simple` module. For the more complex but more flexible API, see
//! the `imageloader` module.

extern crate byteorder;
extern crate flate2;
extern crate libc;

use std::io;

pub mod capi;
pub mod imageloader;
pub mod metadata;
pub mod simple;
mod prediction;

#[cfg(test)]
pub mod test;

#[derive(Debug)]
pub enum PngError {
    Io(io::Error),
    InvalidMetadata(String),
    InvalidScanlinePredictor(u8),
    EntropyDecodingError,
    NoMetadata,
    NoDataProvider,
}

