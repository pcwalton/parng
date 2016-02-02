//! Metadata for PNG images.
//!
//! This is directly taken from the `immeta` library:
//!
//!     https://github.com/netvl/immeta

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
