// parng/lib.rs
//
// Copyright (c) 2016 Mozilla Foundation

//! A simple API that allocates an in-memory buffer and decodes into it.

use PngError;
use imageloader::{self, DataProvider, ImageLoader, InterlacingInfo, LevelOfDetail, LoadProgress};
use imageloader::{ScanlinesForPrediction, ScanlinesForRgbaConversion, UninitializedExtension};
use metadata::ColorType;
use std::io::{Read, Seek};
use std::mem;
use std::sync::mpsc::{self, Receiver, Sender};

struct MemoryDataProvider {
    rgba_pixels: Vec<u8>,
    indexed_pixels: Vec<u8>,
    rgba_aligned_stride: usize,
    indexed_aligned_stride: usize,
    data_sender: Sender<Vec<u8>>,
}

impl MemoryDataProvider {
    #[inline(never)]
    pub fn new(width: u32, height: u32, indexed: bool) -> (MemoryDataProvider, Receiver<Vec<u8>>) {
        let rgba_aligned_stride = imageloader::align(width as usize * 4);
        let indexed_aligned_stride = imageloader::align(width as usize * 4);
        let (data_sender, data_receiver) = mpsc::channel();

        // We make room for eight pixels past the end in case the final scanline consists of a
        // level of detail with a nonzero offset. Tricky!
        let rgba_length = rgba_aligned_stride * (height as usize) + 8 * 4;
        let indexed_length = if indexed {
            indexed_aligned_stride * (height as usize) + 8 + 1
        } else {
            0
        };

        let (mut rgba_pixels, mut indexed_pixels) = (vec![], vec![]);
        unsafe {
            rgba_pixels.extend_with_uninitialized(rgba_length);
            indexed_pixels.extend_with_uninitialized(indexed_length)
        }

        let data_provider = MemoryDataProvider {
            rgba_pixels: rgba_pixels,
            indexed_pixels: indexed_pixels,
            rgba_aligned_stride: rgba_aligned_stride,
            indexed_aligned_stride: indexed_aligned_stride,
            data_sender: data_sender,
        };
        (data_provider, data_receiver)
    }
}

impl DataProvider for MemoryDataProvider {
    fn fetch_scanlines_for_prediction<'a>(&'a mut self,
                                          reference_scanline: Option<u32>,
                                          current_scanline: u32,
                                          lod: LevelOfDetail,
                                          indexed: bool)
                                          -> ScanlinesForPrediction {
        let buffer_color_depth = buffer_color_depth(indexed);
        let reference_scanline = reference_scanline.map(|reference_scanline| {
            InterlacingInfo::new(reference_scanline, buffer_color_depth, lod)
        });
        let current_scanline = InterlacingInfo::new(current_scanline, buffer_color_depth, lod);

        let aligned_stride = if indexed {
            self.indexed_aligned_stride
        } else {
            self.rgba_aligned_stride
        };

        let split_point = aligned_stride * (current_scanline.y as usize);
        let dest_pixels = if indexed {
            &mut self.indexed_pixels
        } else {
            &mut self.rgba_pixels
        };
        let (head, tail) = dest_pixels.split_at_mut(split_point);
        let head_length = head.len();
        let reference_scanline_data = match reference_scanline {
            None => None,
            Some(reference_scanline) => {
                debug_assert!(current_scanline.stride == reference_scanline.stride);
                let start = (reference_scanline.y as usize) * aligned_stride +
                    (reference_scanline.offset as usize);
                let end = start + aligned_stride;
                let slice = &mut head[start..end];
                Some(slice)
            }
        };
        let start = (current_scanline.y as usize) * aligned_stride +
            (current_scanline.offset as usize) - head_length;
        let end = start + aligned_stride;
        let current_scanline_data = &mut tail[start..end];
        ScanlinesForPrediction {
            reference_scanline: reference_scanline_data,
            current_scanline: current_scanline_data,
            stride: current_scanline.stride,
        }
    }

    fn prediction_complete_for_scanline(&mut self, _: u32, _: LevelOfDetail) {}

    fn fetch_scanlines_for_rgba_conversion<'a>(&'a mut self,
                                               scanline: u32,
                                               lod: LevelOfDetail,
                                               indexed: bool)
                                               -> ScanlinesForRgbaConversion<'a> {
        let rgba_scanline = InterlacingInfo::new(scanline, 32, lod);
        let indexed_scanline = if indexed {
            Some(InterlacingInfo::new(scanline, 8, lod))
        } else {
            None
        };
        let rgba_aligned_stride = self.rgba_aligned_stride;
        let indexed_aligned_stride = self.indexed_aligned_stride;
        ScanlinesForRgbaConversion {
            rgba_scanline: &mut self.rgba_pixels[(rgba_aligned_stride * scanline as usize)..],
            indexed_scanline: if indexed_scanline.is_some() {
                Some(&self.indexed_pixels[(indexed_aligned_stride * scanline as usize)..])
            } else {
                None
            },
            rgba_stride: rgba_scanline.stride,
            indexed_stride: indexed_scanline.map(|indexed_scanline| indexed_scanline.stride),
        }
    }

    fn rgba_conversion_complete_for_scanline(&mut self, _: u32, _: LevelOfDetail) {}

    fn finished(&mut self) {
        self.data_sender.send(mem::replace(&mut self.rgba_pixels, vec![])).unwrap()
    }
}

/// An in-memory decoded image in big-endian RGBA format, 32 bits per pixel.
pub struct Image {
    /// The width of the image, in pixels.
    pub width: u32,
    /// The height of the image, in pixels.
    pub height: u32,
    /// The number of bytes between successive scanlines. This may be any value greater than or
    /// equal to `4 * width`.
    ///
    /// Because of SIMD alignment restrictions, `parng` may well choose a value greater than `4 *
    /// width` here.
    pub stride: usize,
    /// The actual pixels.
    pub pixels: Vec<u8>,
}

impl Image {
    /// Allocates space for and loads a PNG image stream from a reader into memory.
    ///
    /// The returned image is big-endian, 32 bits per pixel RGBA.
    ///
    /// This method does not return until the image is fully loaded. If you need a different
    /// in-memory representation, or you need to display the image before it's fully loaded,
    /// consider using the `imageloader::ImageLoader` API instead.
    pub fn load<I>(input: &mut I) -> Result<Image, PngError> where I: Read + Seek {
        let mut image = ImageLoader::new();
        loop {
            match try!(image.add_data(input)) {
                LoadProgress::NeedDataProviderAndMoreData => break,
                LoadProgress::NeedMoreData => {}
                LoadProgress::Finished => panic!("Image ended before metadata was read!"),
            }
        }

        let (dimensions, indexed) = {
            let metadata = image.metadata().as_ref().unwrap();
            (metadata.dimensions, metadata.color_type == ColorType::Indexed)
        };
        let (data_provider, data_receiver) = MemoryDataProvider::new(dimensions.width,
                                                                     dimensions.height,
                                                                     indexed);
        let aligned_stride = data_provider.rgba_aligned_stride;
        image.set_data_provider(Box::new(data_provider));

        while let LoadProgress::NeedMoreData = try!(image.add_data(input)) {}
        try!(image.wait_until_finished());

        let pixels = data_receiver.recv().unwrap();
        Ok(Image {
            width: dimensions.width,
            height: dimensions.height,
            stride: aligned_stride,
            pixels: pixels,
        })
    }
}

fn buffer_color_depth(indexed: bool) -> u8 {
    if indexed {
        8
    } else {
        32
    }
}

