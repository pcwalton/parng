//! Metadata for PNG images.
//!
//! This code is derived from code in the `immeta` library: https://github.com/netvl/immeta

use PngError;
use byteorder::{self, ReadBytesExt, BigEndian};
use num::ToPrimitive;
use std::io::Read;

/// Represents image dimensions in pixels.
///
/// It is possible to convert pairs of type `(T1, T2)`, where `T1` and `T2` are primitive
/// number types, to this type, however, this is mostly needed for internal usage.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Dimensions {
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
}

impl<T: ToPrimitive, U: ToPrimitive> From<(T, U)> for Dimensions {
    fn from((w, h): (T, U)) -> Dimensions {
        Dimensions {
            width: w.to_u32().unwrap(),
            height: h.to_u32().unwrap()
        }
    }
}

/// Color type used in an image.
///
/// These color types directly corresponds to those defined in PNG spec.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum ColorType {
    Grayscale,
    Rgb,
    Indexed,
    GrayscaleAlpha,
    RgbAlpha
}

const CT_GRAYSCALE: u8 = 0;
const CT_RGB: u8 = 2;
const CT_INDEXED: u8 = 3;
const CT_GRAYSCALE_ALPHA: u8 = 4;
const CT_RGB_ALPHA: u8 = 6;

impl ColorType {
    fn from_u8(n: u8) -> Option<ColorType> {
        match n {
            CT_GRAYSCALE       => Some(ColorType::Grayscale),
            CT_RGB             => Some(ColorType::Rgb),
            CT_INDEXED         => Some(ColorType::Indexed),
            CT_GRAYSCALE_ALPHA => Some(ColorType::GrayscaleAlpha),
            CT_RGB_ALPHA       => Some(ColorType::RgbAlpha),
            _                  => None
        }
    }
}

fn compute_color_depth(bit_depth: u8, color_type: u8) -> Option<u8> {
    match color_type {
        CT_INDEXED => match bit_depth {
            1 | 2 | 4 | 8 => Some(bit_depth),
            _ => None
        },
        CT_GRAYSCALE => match bit_depth {
            1 | 2 | 4 | 8 | 16 => Some(bit_depth),
            _ => None,
        },
        CT_GRAYSCALE_ALPHA => match bit_depth {
            8 | 16 => Some(bit_depth*2),
            _ => None
        },
        CT_RGB => match bit_depth {
            8 | 16 => Some(bit_depth*3),
            _ => None
        },
        CT_RGB_ALPHA => match bit_depth {
            8 | 16 => Some(bit_depth*4),
            _ => None
        },
        _ => None
    }
}

/// Compression method used in an image.
///
/// PNG spec currently defines only one compression method:
///
/// > At present, only compression method 0 (deflate/inflate compression with a sliding window of
/// at most 32768 bytes) is defined.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum CompressionMethod {
    DeflateInflate
}

impl CompressionMethod {
    fn from_u8(n: u8) -> Option<CompressionMethod> {
        match n {
            0 => Some(CompressionMethod::DeflateInflate),
            _ => None
        }
    }
}

/// Filtering method used in an image.
///
/// PNG spec currently defines only one filter method:
///
/// > At present, only filter method 0 (adaptive filtering with five basic filter types) is
/// defined.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum FilterMethod {
    AdaptiveFiltering
}

impl FilterMethod {
    fn from_u8(n: u8) -> Option<FilterMethod> {
        match n {
            0 => Some(FilterMethod::AdaptiveFiltering),
            _ => None
        }
    }
}

/// Interlace method used in an image.
///
/// PNG spec says that interlacing can be disabled or Adam7 interlace method can be used.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum InterlaceMethod {
    Disabled,
    Adam7
}

impl InterlaceMethod {
    fn from_u8(n: u8) -> Option<InterlaceMethod> {
        match n {
            0 => Some(InterlaceMethod::Disabled),
            1 => Some(InterlaceMethod::Adam7),
            _ => None
        }
    }
}

/// Represents a PNG chunk.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ChunkHeader {
    pub length: u32,
    pub chunk_type: [u8; 4],
}

impl ChunkHeader {
    pub fn load<R: ?Sized + Read>(reader: &mut R) -> Result<ChunkHeader,PngError> {
        // chunk length
        let length = try!(reader.read_u32::<BigEndian>()
                                .map_byteorder_error("when reading chunk length"));
        
        let mut chunk_type = [0u8; 4];
        try!(reader.read_exact(&mut chunk_type)
                   .map_err(|_| format_eof("when reading chunk type")));

        Ok(ChunkHeader {
            length: length,
            chunk_type: chunk_type,
        })
    }
}

/// Represents metadata of a PNG image.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Metadata {
    /// Width and height.
    pub dimensions: Dimensions,
    /// Color type used in the image.
    pub color_type: ColorType,
    /// Color depth (bits per pixel) used in the image.
    pub color_depth: u8,
    /// Compression method used in the image.
    pub compression_method: CompressionMethod,
    /// Preprocessing method used in the image.
    pub filter_method: FilterMethod,
    /// Transmission order used in the image.
    pub interlace_method: InterlaceMethod
}

impl Metadata {
    pub fn load<R: ?Sized + Read>(r: &mut R) -> Result<Metadata,PngError> {
        let mut signature = [0u8; 8];
        try!(r.read_exact(&mut signature).map_err(|_| format_eof("when reading PNG signature")));

        if &signature != b"\x89PNG\r\n\x1a\n" {
            return Err(PngError::InvalidMetadata(format!("invalid PNG header: {:?}", signature)));
        }

        // chunk length
        let chunk_header = try!(ChunkHeader::load(r));
        if &chunk_header.chunk_type != b"IHDR" {
            return Err(PngError::InvalidMetadata(format!("invalid PNG chunk: {:?}",
                                                         chunk_header.chunk_type)));
        }

        let width = try!(r.read_u32::<BigEndian>().map_byteorder_error("when reading width"));
        let height = try!(r.read_u32::<BigEndian>().map_byteorder_error("when reading height"));
        let bit_depth = try!(r.read_u8().map_byteorder_error("when reading bit depth"));
        let color_type = try!(r.read_u8().map_byteorder_error("when reading color type"));
        let compression_method =
            try!(r.read_u8().map_byteorder_error("when reading compression method"));
        let filter_method = try!(r.read_u8().map_byteorder_error("when reading filter method"));
        let interlace_method =
            try!(r.read_u8().map_byteorder_error("when reading interlace method"));

        drop(try!(r.read_u32::<BigEndian>().map_byteorder_error("when reading metadata CRC")));

        Ok(Metadata {
            dimensions: (width, height).into(),
            color_type: try!(
                ColorType::from_u8(color_type).ok_or(
                    PngError::InvalidMetadata(format!("invalid color type: {}", color_type)))
            ),
            color_depth: try!(
                compute_color_depth(bit_depth, color_type).ok_or(
                    PngError::InvalidMetadata(format!("invalid bit depth: {}", bit_depth)))
            ),
            compression_method: try!(
                CompressionMethod::from_u8(compression_method).ok_or(PngError::InvalidMetadata(
                        format!("invalid compression method: {}", compression_method)))
            ),
            filter_method: try!(
                FilterMethod::from_u8(filter_method).ok_or(PngError::InvalidMetadata(
                        format!("invalid filter method: {}", filter_method)))
            ),
            interlace_method: try!(
                InterlaceMethod::from_u8(interlace_method).ok_or(PngError::InvalidMetadata(
                        format!("invalid interlace method: {}", interlace_method)))
            )
        })
    }
}

trait MapByteOrderError {
    type OkType;
    fn map_byteorder_error(self, description: &'static str) -> Result<Self::OkType,PngError>;
}

impl<T> MapByteOrderError for byteorder::Result<T> {
    type OkType = T;
    fn map_byteorder_error(self, description: &'static str) -> Result<Self::OkType,PngError> {
        match self {
            Err(byteorder::Error::Io(io_error)) => Err(PngError::Io(io_error)),
            Err(byteorder::Error::UnexpectedEOF) => Err(format_eof(description)),
            Ok(value) => Ok(value),
        }
    }
}

fn format_eof(description: &'static str) -> PngError {
    PngError::InvalidMetadata(format!("unexpected end of file {}", description))
}

