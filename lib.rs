// parng/lib.rs
//
// Copyright (c) 2016 Mozilla Foundation

//! A parallel PNG decoder.

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
    NeedMoreData,
    Io(io::Error),
    InvalidMetadata(String),
    InvalidOperation(&'static str),
    InvalidData(String),
    InvalidScanlinePredictor(u8),
    EntropyDecodingError,
    NoMetadata,
    NoDataProvider,
}

