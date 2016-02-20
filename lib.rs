// parng/lib.rs
//
// Copyright (c) 2016 Mozilla Foundation

//! A parallel PNG decoder.

extern crate byteorder;
extern crate libc;
extern crate libz_sys;

pub mod capi;
pub mod imageloader;
pub mod metadata;
pub mod simple;
mod prediction;

use libc::c_int;
use std::io;

#[derive(Debug)]
pub enum PngError {
    NeedMoreData,
    Io(io::Error),
    InvalidMetadata(String),
    InvalidOperation(&'static str),
    InvalidData(String),
    InvalidScanlinePredictor(u8),
    EntropyDecodingError(c_int),
    NoMetadata,
    NoDataProvider,
}

