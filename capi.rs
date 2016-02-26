// parng/capi.rs
//
// Copyright (c) 2016 Mozilla Foundation

//! A C API to `parng`.

#![allow(non_camel_case_types)]

use PngError;
use imageloader::{self, DataProvider, ImageLoader, InterlacingInfo, LevelOfDetail, LoadProgress};
use imageloader::{ScanlinesForPrediction, ScanlinesForRgbaConversion};
use libc::{self, FILE, SEEK_CUR, SEEK_END, SEEK_SET, c_void, size_t, uintptr_t};
use metadata::{ColorType, InterlaceMethod, Metadata};
use simple::Image;
use std::io::{self, Cursor, Error, ErrorKind, Read, Seek, SeekFrom};
use std::mem;
use std::ptr;
use std::slice;

/// See `metadata::ColorType`.
pub type parng_color_type = u32;
/// See `metadata::CompressionMethod`.
pub type parng_compression_method = u32;
/// See `PngError`.
pub type parng_error = u32;
/// See `metadata::FilterMethod`.
pub type parng_filter_method = u32;
/// See `imageloader::ImageLoader`.
pub type parng_image_loader = ImageLoader;
/// See `metadata::InterlaceMethod`.
pub type parng_interlace_method = u32;
/// See `std::io::Error`.
pub type parng_io_error = u32;
/// See `imageloader::LevelOfDetail`.
pub type parng_level_of_detail = i32;
/// See `imageloader::LoadProgress`.
pub type parng_load_progress = u32;
/// See `std::io::SeekFrom`.
pub type parng_seek_from = u32;

pub const PARNG_LOAD_PROGRESS_FINISHED: u32 = 0;
pub const PARNG_LOAD_PROGRESS_NEED_MORE_DATA: u32 = 1;
pub const PARNG_LOAD_PROGRESS_NEED_DATA_PROVIDER_AND_MORE_DATA: u32 = 2;

pub const PARNG_COLOR_TYPE_GRAYSCALE: u32 = 0;
pub const PARNG_COLOR_TYPE_RGB: u32 = 2;
pub const PARNG_COLOR_TYPE_INDEXED: u32 = 3;
pub const PARNG_COLOR_TYPE_GRAYSCALE_ALPHA: u32 = 4;
pub const PARNG_COLOR_TYPE_RGB_ALPHA: u32 = 5;

pub const PARNG_COMPRESSION_METHOD_DEFLATE: u32 = 0;

pub const PARNG_SUCCESS: u32 = 0;
pub const PARNG_ERROR_IO: u32 = 1;
pub const PARNG_ERROR_INVALID_METADATA: u32 = 2;
pub const PARNG_ERROR_INVALID_SCANLINE_PREDICTOR: u32 = 3;
pub const PARNG_ERROR_ENTROPY_DECODING_ERROR: u32 = 4;
pub const PARNG_ERROR_NO_DATA_PROVIDER: u32 = 5;

pub const PARNG_FILTER_METHOD_ADAPTIVE: u32 = 0;

pub const PARNG_INTERLACE_METHOD_NONE: u32 = 0;
pub const PARNG_INTERLACE_METHOD_ADAM7: u32 = 1;

pub const PARNG_LEVEL_OF_DETAIL_NONE: i32 = -1;
pub const PARNG_LEVEL_OF_DETAIL_ADAM7_0: i32 = 0;
pub const PARNG_LEVEL_OF_DETAIL_ADAM7_1: i32 = 1;
pub const PARNG_LEVEL_OF_DETAIL_ADAM7_2: i32 = 2;
pub const PARNG_LEVEL_OF_DETAIL_ADAM7_3: i32 = 3;
pub const PARNG_LEVEL_OF_DETAIL_ADAM7_4: i32 = 4;
pub const PARNG_LEVEL_OF_DETAIL_ADAM7_5: i32 = 5;
pub const PARNG_LEVEL_OF_DETAIL_ADAM7_6: i32 = 6;

pub const PARNG_SEEK_FROM_START: u32 = 0;
pub const PARNG_SEEK_FROM_CURRENT: u32 = 1;
pub const PARNG_SEEK_FROM_END: u32 = 2;

/// The fields of this structure are intentionally private so that the rest of `parng` can't
/// violate memory safety.
#[repr(C)]
pub struct parng_reader {
    read: unsafe extern "C" fn(buffer: *mut u8,
                               buffer_length: size_t,
                               bytes_read: *mut size_t,
                               user_data: *mut c_void)
                               -> parng_io_error,
    seek: unsafe extern "C" fn(position: i64,
                               from: parng_seek_from,
                               new_position: *mut u64,
                               user_data: *mut c_void)
                               -> parng_io_error,
    user_data: *mut c_void,
}

impl Read for parng_reader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        unsafe {
            let mut bytes_read = 0;
            match (self.read)(buffer.as_mut_ptr(), buffer.len(), &mut bytes_read, self.user_data) {
                PARNG_SUCCESS => Ok(bytes_read),
                PARNG_ERROR_IO => Err(Error::new(ErrorKind::Other, "`parng` reader error")),
                _ => {
                    panic!("`parng_reader::read()` must return either `PARNG_SUCCESS` or \
                            `PARNG_ERROR_IO`!")
                }
            }
        }
    }
}

impl Seek for parng_reader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        unsafe {
            let (seek_from, position) = match pos {
                SeekFrom::Start(position) => (PARNG_SEEK_FROM_START, position as i64),
                SeekFrom::Current(position) => (PARNG_SEEK_FROM_CURRENT, position),
                SeekFrom::End(position) => (PARNG_SEEK_FROM_END, position),
            };
            let mut new_position = 0;
            match (self.seek)(position, seek_from, &mut new_position, self.user_data) {
                PARNG_SUCCESS => Ok(new_position),
                PARNG_ERROR_IO => Err(Error::new(ErrorKind::Other, "`parng` reader error")),
                _ => {
                    panic!("`parng_reader::seek()` must return either `PARNG_SUCCESS` or \
                            `PARNG_ERROR_IO`!")
                }
            }
        }
    }
}

struct FileReader {
    file: *mut FILE,
}

impl Read for FileReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        unsafe {
            let nread = libc::fread(buffer.as_mut_ptr() as *mut c_void,
                                    1,
                                    buffer.len(),
                                    self.file);
            if nread > 0 {
                return Ok(nread)
            }
            if libc::ferror(self.file) == 0 {
                Ok(0)
            } else {
                Err(Error::last_os_error())
            }
        }
    }
}

impl Seek for FileReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let (offset, whence) = match pos {
            SeekFrom::Start(offset) => (offset as i64, SEEK_SET),
            SeekFrom::End(offset) => (offset as i64, SEEK_END),
            SeekFrom::Current(offset) => (offset as i64, SEEK_CUR),
        };
        unsafe {
            let new_offset = libc::fseek(self.file, offset, whence);
            Ok(new_offset as u64)
        }
    }
}

#[repr(C)]
pub struct parng_scanlines_for_prediction {
    pub reference_scanline: *mut u8,
    pub reference_scanline_length: size_t,
    pub current_scanline: *mut u8,
    pub current_scanline_length: size_t,
    pub stride: u8,
}

#[repr(C)]
pub struct parng_scanlines_for_rgba_conversion {
    pub rgba_scanline: *mut u8,
    pub rgba_scanline_length: size_t,
    pub indexed_scanline: *const u8,
    pub indexed_scanline_length: size_t,
    pub rgba_stride: u8,
    pub indexed_stride: u8,
}

/// The fields of this structure are intentionally private so that the rest of `parng` can't
/// violate memory safety.
#[repr(C)]
pub struct parng_image {
    width: u32,
    height: u32,
    stride: size_t,
    capacity: size_t,
    pixels: *mut u8,
}

/// The fields of this structure are intentionally private so that the rest of `parng` can't
/// violate memory safety.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct parng_data_provider {
    fetch_scanlines_for_prediction: extern "C" fn(reference_scanline: i32,
                                                  current_scanline: u32,
                                                  lod: parng_level_of_detail,
                                                  indexed: i32,
                                                  scanlines: *mut parng_scanlines_for_prediction,
                                                  user_data: *mut c_void),
    prediction_complete_for_scanline: extern "C" fn(scanline: u32,
                                                    lod: parng_level_of_detail,
                                                    user_data: *mut c_void),
    fetch_scanlines_for_rgba_conversion:
        extern "C" fn(scanline: u32,
                      lod: parng_level_of_detail,
                      scanlines: *mut parng_scanlines_for_rgba_conversion,
                      user_data: *mut c_void),
    rgba_conversion_complete_for_scanline: extern "C" fn(scanline: u32,
                                                         lod: parng_level_of_detail,
                                                         user_data: *mut c_void),
    finished: extern "C" fn(user_data: *mut c_void),
    user_data: *mut c_void,
}

unsafe impl Send for parng_data_provider {}

impl DataProvider for parng_data_provider {
    fn fetch_scanlines_for_prediction<'a>(&'a mut self,
                                          reference_scanline: Option<u32>,
                                          current_scanline: u32,
                                          lod: LevelOfDetail,
                                          indexed: bool)
                                          -> ScanlinesForPrediction<'a> {
        unsafe {
            let mut c_scanlines_for_prediction = parng_scanlines_for_prediction {
                reference_scanline: ptr::null_mut(),
                reference_scanline_length: 0,
                current_scanline: ptr::null_mut(),
                current_scanline_length: 0,
                stride: 0,
            };
            let c_reference_scanline = match reference_scanline {
                None => -1,
                Some(reference_scanline) => reference_scanline as i32,
            };
            let c_lod = level_of_detail_to_c_level_of_detail(lod);
            let c_indexed = if indexed {
                1
            } else {
                0
            };
            (self.fetch_scanlines_for_prediction)(c_reference_scanline,
                                                  current_scanline,
                                                  c_lod,
                                                  c_indexed,
                                                  &mut c_scanlines_for_prediction,
                                                  self.user_data);
            c_scanlines_for_prediction_to_scanlines_for_prediction(&c_scanlines_for_prediction)
        }
    }

    fn prediction_complete_for_scanline(&mut self, scanline: u32, lod: LevelOfDetail) {
        let c_lod = level_of_detail_to_c_level_of_detail(lod);
        (self.prediction_complete_for_scanline)(scanline, c_lod, self.user_data);
    }

    fn fetch_scanlines_for_rgba_conversion<'a>(&'a mut self, scanline: u32, lod: LevelOfDetail)
                                               -> ScanlinesForRgbaConversion<'a> {
       unsafe {
           let mut c_scanlines_for_rgba_conversion = parng_scanlines_for_rgba_conversion {
               rgba_scanline: ptr::null_mut(),
               rgba_scanline_length: 0,
               indexed_scanline: ptr::null(),
               indexed_scanline_length: 0,
               rgba_stride: 0,
               indexed_stride: 0,
           };
           let c_lod = level_of_detail_to_c_level_of_detail(lod);
           (self.fetch_scanlines_for_rgba_conversion)(scanline,
                                                      c_lod,
                                                      &mut c_scanlines_for_rgba_conversion,
                                                      self.user_data);
           c_scanlines_for_rgba_conversion_to_scanlines_for_rgba_conversion(
               &c_scanlines_for_rgba_conversion)
       }
    }

    fn rgba_conversion_complete_for_scanline(&mut self, scanline: u32, lod: LevelOfDetail) {
        let c_lod = level_of_detail_to_c_level_of_detail(lod);
        (self.rgba_conversion_complete_for_scanline)(scanline, c_lod, self.user_data);
    }

    fn finished(&mut self) {
        (self.finished)(self.user_data)
    }
}

#[repr(C)]
pub struct parng_metadata {
    pub width: u32,
    pub height: u32,
    pub color_type: parng_color_type,
    pub compression_method: parng_compression_method,
    pub filter_method: parng_filter_method,
    pub interlace_method: parng_interlace_method,
}

#[repr(C)]
pub struct parng_interlacing_info {
    pub y: u32,
    pub stride: u8,
    pub offset: u8,
}

#[no_mangle]
pub unsafe extern "C" fn parng_image_load(c_image: *mut parng_image, reader: *mut parng_reader)
                                          -> parng_error {
    match Image::load(&mut *reader) {
        Err(error) => png_error_to_c_error(error),
        Ok(image) => {
            *c_image = image_to_c_image(image);
            PARNG_SUCCESS
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn parng_image_load_from_file(c_image: *mut parng_image, file: *mut FILE)
                                                    -> parng_error {
    let mut file_reader = FileReader {
        file: file,
    };
    match Image::load(&mut file_reader) {
        Err(error) => png_error_to_c_error(error),
        Ok(image) => {
            *c_image = image_to_c_image(image);
            PARNG_SUCCESS
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn parng_image_load_from_memory(c_image: *mut parng_image,
                                                      bytes: *const u8,
                                                      length: size_t)
                                                      -> parng_error {
    match Image::load(&mut Cursor::new(slice::from_raw_parts(bytes, length))) {
        Err(error) => png_error_to_c_error(error),
        Ok(image) => {
            *c_image = image_to_c_image(image);
            PARNG_SUCCESS
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn parng_image_destroy(image: *mut parng_image) {
    drop(Vec::from_raw_parts((*image).pixels,
                             (*image).stride * (*image).height as usize,
                             (*image).capacity))
}

#[no_mangle]
pub unsafe extern "C" fn parng_image_loader_create(image_loader: *mut *mut parng_image_loader) {
    let new_image_loader = ImageLoader::new();
    *image_loader = mem::transmute::<Box<ImageLoader>,
                                     *mut ImageLoader>(Box::new(new_image_loader));
}

#[no_mangle]
pub unsafe extern "C" fn parng_image_loader_destroy(image_loader: *mut parng_image_loader) {
    drop(mem::transmute::<*mut parng_image_loader, Box<ImageLoader>>(image_loader))
}

#[no_mangle]
pub unsafe extern "C" fn parng_image_loader_add_data(image_loader: *mut parng_image_loader,
                                                     reader: *mut parng_reader,
                                                     result: *mut parng_load_progress)
                                                     -> parng_error {
    match (*image_loader).add_data(&mut *reader) {
        Ok(load_progress) => {
            *result = load_progress_to_c_result(load_progress);
            PARNG_SUCCESS
        }
        Err(err) => png_error_to_c_error(err)
    }
}

#[no_mangle]
pub unsafe extern "C" fn parng_image_loader_wait_until_finished(
        image_loader: *mut parng_image_loader)
        -> parng_error {
    match (*image_loader).wait_until_finished() {
        Ok(()) => PARNG_SUCCESS,
        Err(err) => png_error_to_c_error(err),
    }
}


#[no_mangle]
pub unsafe extern "C" fn parng_image_loader_set_data_provider(
        image_loader: *mut parng_image_loader,
        data_provider: *mut parng_data_provider) {
    (*image_loader).set_data_provider(Box::new(*data_provider))
}

#[no_mangle]
pub unsafe extern "C" fn parng_image_loader_get_metadata(image_loader: *mut parng_image_loader,
                                                         metadata_result: *mut parng_metadata)
                                                         -> u32 {
    match *(*image_loader).metadata() {
        None => 0,
        Some(ref metadata) => {
            *metadata_result = metadata_to_c_metadata(metadata);
            1
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn parng_image_loader_align(address: uintptr_t) -> uintptr_t {
    imageloader::align(address)
}

#[no_mangle]
pub unsafe extern "C" fn parng_interlacing_info_init(
        interlacing_info: *mut parng_interlacing_info,
        y: u32,
        color_depth: u8,
        lod: parng_level_of_detail) {
    let info = InterlacingInfo::new(y, color_depth, c_level_of_detail_to_level_of_detail(lod));
    (*interlacing_info).y = info.y;
    (*interlacing_info).stride = info.stride;
    (*interlacing_info).offset = info.offset
}

fn png_error_to_c_error(err: PngError) -> parng_error {
    match err {
        PngError::Io(_) => PARNG_ERROR_IO,
        PngError::InvalidMetadata(_) => PARNG_ERROR_INVALID_METADATA,
        PngError::InvalidScanlinePredictor(_) => PARNG_ERROR_INVALID_SCANLINE_PREDICTOR,
        PngError::EntropyDecodingError => PARNG_ERROR_ENTROPY_DECODING_ERROR,
        PngError::NoDataProvider => PARNG_ERROR_NO_DATA_PROVIDER,
    }
}

fn load_progress_to_c_result(result: LoadProgress) -> parng_load_progress {
    match result {
        LoadProgress::Finished => PARNG_LOAD_PROGRESS_FINISHED,
        LoadProgress::NeedMoreData => PARNG_LOAD_PROGRESS_NEED_MORE_DATA,
        LoadProgress::NeedDataProviderAndMoreData => {
            PARNG_LOAD_PROGRESS_NEED_DATA_PROVIDER_AND_MORE_DATA
        }
    }
}

fn level_of_detail_to_c_level_of_detail(lod: LevelOfDetail) -> parng_level_of_detail {
    match lod {
        LevelOfDetail::None => PARNG_LEVEL_OF_DETAIL_NONE,
        LevelOfDetail::Adam7(level) => PARNG_LEVEL_OF_DETAIL_ADAM7_0 + level as i32,
    }
}

fn c_level_of_detail_to_level_of_detail(c_lod: parng_level_of_detail) -> LevelOfDetail {
    match c_lod {
        PARNG_LEVEL_OF_DETAIL_NONE => LevelOfDetail::None,
        _ if c_lod >= PARNG_LEVEL_OF_DETAIL_ADAM7_0 && c_lod <= PARNG_LEVEL_OF_DETAIL_ADAM7_6 => {
            LevelOfDetail::Adam7((c_lod - PARNG_LEVEL_OF_DETAIL_ADAM7_0) as u8)
        }
        _ => panic!("Not a valid level of detail!"),
    }
}

unsafe fn c_scanlines_for_prediction_to_scanlines_for_prediction(
        c_scanlines_for_prediction: *const parng_scanlines_for_prediction)
        -> ScanlinesForPrediction<'static> {
    ScanlinesForPrediction {
        reference_scanline: if (*c_scanlines_for_prediction).reference_scanline.is_null() {
            None
        } else {
            Some(slice::from_raw_parts_mut(
                    (*c_scanlines_for_prediction).reference_scanline,
                    (*c_scanlines_for_prediction).reference_scanline_length))
        },
        current_scanline: slice::from_raw_parts_mut(
                              (*c_scanlines_for_prediction).current_scanline,
                              (*c_scanlines_for_prediction).current_scanline_length),
        stride: (*c_scanlines_for_prediction).stride,
    }
}

unsafe fn c_scanlines_for_rgba_conversion_to_scanlines_for_rgba_conversion(
        c_scanlines_for_rgba_conversion: *const parng_scanlines_for_rgba_conversion)
         -> ScanlinesForRgbaConversion<'static> {
    ScanlinesForRgbaConversion {
        rgba_scanline: slice::from_raw_parts_mut(
                           (*c_scanlines_for_rgba_conversion).rgba_scanline,
                           (*c_scanlines_for_rgba_conversion).rgba_scanline_length),
        indexed_scanline: slice::from_raw_parts(
            (*c_scanlines_for_rgba_conversion).indexed_scanline,
            (*c_scanlines_for_rgba_conversion).indexed_scanline_length),
        rgba_stride: (*c_scanlines_for_rgba_conversion).rgba_stride,
        indexed_stride: (*c_scanlines_for_rgba_conversion).indexed_stride,
    }
}

fn metadata_to_c_metadata(metadata: &Metadata) -> parng_metadata {
    parng_metadata {
        width: metadata.dimensions.width,
        height: metadata.dimensions.height,
        color_type: color_type_to_c_color_type(metadata.color_type),
        compression_method: PARNG_COMPRESSION_METHOD_DEFLATE,
        filter_method: PARNG_FILTER_METHOD_ADAPTIVE,
        interlace_method: interlace_method_to_c_interlace_method(metadata.interlace_method),
    }
}

fn color_type_to_c_color_type(color_type: ColorType) -> parng_color_type {
    match color_type {
        ColorType::Grayscale => PARNG_COLOR_TYPE_GRAYSCALE,
        ColorType::Rgb => PARNG_COLOR_TYPE_RGB,
        ColorType::Indexed => PARNG_COLOR_TYPE_INDEXED,
        ColorType::GrayscaleAlpha => PARNG_COLOR_TYPE_GRAYSCALE_ALPHA,
        ColorType::RgbAlpha => PARNG_COLOR_TYPE_RGB_ALPHA,
    }
}

fn interlace_method_to_c_interlace_method(interlace_method: InterlaceMethod)
                                          -> parng_interlace_method {
    match interlace_method {
        InterlaceMethod::Disabled => PARNG_INTERLACE_METHOD_NONE,
        InterlaceMethod::Adam7 => PARNG_INTERLACE_METHOD_ADAM7,
    }
}

unsafe fn image_to_c_image(mut image: Image) -> parng_image {
    assert!(image.stride * image.height as usize == image.pixels.len());
    let c_image = parng_image {
        width: image.width,
        height: image.height,
        stride: image.stride,
        capacity: image.pixels.capacity(),
        pixels: image.pixels.as_mut_ptr(),
    };
    mem::forget(image.pixels);
    c_image
}

