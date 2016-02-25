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

/// Errors that can occur while decoding a PNG image.
#[derive(Debug)]
pub enum PngError {
    /// A Rust I/O error occurred. The wrapped error contains detailed information about the error.
    Io(io::Error),
    /// The image loader found image data to decode, but no data provider was attached.
    ///
    /// When `ImageLoader::add_data()` returns, if `ImageLoader::metadata()` returns a `Some`
    /// value, a data provider must be attached to the image loader via
    /// `ImageLoader::set_data_provider()` before `ImageLoader::add_data()` can be successfully
    /// called again. If this is not done, this error will be returned.
    NoDataProvider,
    /// The PNG image had an invalid header. The string contains detailed information about the
    /// error.
    InvalidMetadata(String),
    /// An invalid scanline predictor was specified. This indicates corrupt image data.
    InvalidScanlinePredictor(u8),
    /// The entropy decoding (`zlib` decompression) failed. This indicates corrupt image data.
    EntropyDecodingError,
}

