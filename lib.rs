// parng/lib.rs
//
// Copyright (c) 2016 Mozilla Foundation

//! A parallel PNG decoder.

extern crate byteorder;
extern crate libc;
extern crate libz_sys;
extern crate num;

use libc::c_int;
use libz_sys::{Z_ERRNO, Z_NO_FLUSH, Z_OK, Z_STREAM_END, z_stream};
use metadata::{ChunkHeader, ColorType, InterlaceMethod, Metadata};
use prediction::{MainThreadToPredictorThreadComm, MainThreadToPredictorThreadMsg};
use prediction::{PredictionRequest, Predictor, PredictorThreadToMainThreadMsg};
use std::cmp;
use std::io::{self, Read, Seek, SeekFrom};
use std::mem;

const BUFFER_SIZE: usize = 16384;

pub mod metadata;
mod prediction;

pub struct Image {
    metadata: Option<Metadata>,
    z_stream: z_stream,
    compressed_data_buffer: Vec<u8>,
    compressed_data_consumed: usize,
    palette: Vec<u8>,
    scanline_data_buffer: Vec<u8>,
    scanline_data_buffer_size: usize,
    cached_scanline_data_buffers: Vec<Vec<u8>>,

    /// If `None`, the buffered scanline isn't full yet.
    scanline_data_buffer_info: Option<BufferedScanlineInfo>,

    current_y: u32,
    current_lod: LevelOfDetail,
    scanlines_decoded_in_this_lod: u32,
    last_decoded_lod: LevelOfDetail,
    decode_state: DecodeState,
    predictor_thread_comm: MainThreadToPredictorThreadComm,
}

impl Drop for Image {
    fn drop(&mut self) {
        unsafe {
            drop(libz_sys::deflateEnd(&mut self.z_stream))
        }
    }
}

impl Image {
    pub fn new() -> Result<Image,PngError> {
        let mut z_stream;
        unsafe {
            z_stream = mem::zeroed();
            try!(PngError::from_zlib_result(inflateInit(&mut z_stream)))
        }
        Ok(Image {
            metadata: None,
            z_stream: z_stream,
            compressed_data_buffer: vec![],
            compressed_data_consumed: 0,
            palette: vec![],
            scanline_data_buffer: vec![],
            scanline_data_buffer_size: 0,
            scanline_data_buffer_info: None,
            cached_scanline_data_buffers: vec![],
            current_y: 0,
            current_lod: LevelOfDetail::None,
            scanlines_decoded_in_this_lod: 0,
            last_decoded_lod: LevelOfDetail::None,
            decode_state: DecodeState::Start,
            predictor_thread_comm: MainThreadToPredictorThreadComm::new(),
        })
    }

    #[inline(never)]
    pub fn add_data<R>(&mut self, reader: &mut R) -> Result<AddDataResult,PngError>
                       where R: Read + Seek {
        loop {
            match self.decode_state {
                DecodeState::Start => {
                    let initial_pos =
                        try!(reader.seek(SeekFrom::Current(0)).map_err(PngError::Io));
                    match Metadata::load(reader) {
                        Ok(metadata) => {
                            self.current_lod = match metadata.interlace_method {
                                InterlaceMethod::Adam7 => LevelOfDetail::Adam7(0),
                                InterlaceMethod::Disabled => LevelOfDetail::None,
                            };

                            self.decode_state = if metadata.color_type == ColorType::Indexed {
                                DecodeState::LookingForPalette
                            } else {
                                DecodeState::LookingForImageData
                            };

                            self.metadata = Some(metadata)
                        }
                        Err(PngError::NeedMoreData) => {
                            try!(reader.seek(SeekFrom::Start(initial_pos)).map_err(PngError::Io));
                            return Ok(AddDataResult::Continue)
                        }
                        Err(error) => return Err(error),
                    }
                }
                DecodeState::LookingForPalette => {
                    let initial_pos =
                        try!(reader.seek(SeekFrom::Current(0)).map_err(PngError::Io));
                    let chunk_header = match ChunkHeader::load(reader) {
                        Err(PngError::NeedMoreData) => {
                            try!(reader.seek(SeekFrom::Start(initial_pos)).map_err(PngError::Io));
                            return Ok(AddDataResult::Continue)
                        }
                        Err(error) => return Err(error),
                        Ok(chunk_header) => chunk_header,
                    };
                    if &chunk_header.chunk_type == b"PLTE" {
                        self.decode_state = DecodeState::ReadingPalette(chunk_header.length);
                    } else {
                        // Skip over this chunk, adding 4 to move past the CRC.
                        try!(reader.seek(SeekFrom::Current((chunk_header.length as i64) + 4))
                                   .map_err(PngError::Io));
                    }
                }
                DecodeState::LookingForImageData => {
                    let initial_pos =
                        try!(reader.seek(SeekFrom::Current(0)).map_err(PngError::Io));
                    let chunk_header = match ChunkHeader::load(reader) {
                        Err(PngError::NeedMoreData) => {
                            try!(reader.seek(SeekFrom::Start(initial_pos)).map_err(PngError::Io));
                            return Ok(AddDataResult::Continue)
                        }
                        Err(error) => return Err(error),
                        Ok(chunk_header) => chunk_header,
                    };
                    if &chunk_header.chunk_type == b"IDAT" {
                        self.decode_state = DecodeState::DecodingData(chunk_header.length);
                    } else if &chunk_header.chunk_type == b"IEND" {
                        self.decode_state = DecodeState::Finished
                    } else {
                        // Skip over this chunk, adding 4 to move past the CRC.
                        try!(reader.seek(SeekFrom::Current((chunk_header.length as i64) + 4))
                                   .map_err(PngError::Io));
                    }
                }
                DecodeState::ReadingPalette(mut bytes_left_in_chunk) => {
                    let original_palette_size = self.palette.len();
                    self.palette.resize(original_palette_size + bytes_left_in_chunk as usize, 0);
                    let bytes_read =
                        try!(reader.read(&mut self.palette[original_palette_size..])
                                   .map_err(PngError::Io));
                    bytes_left_in_chunk -= bytes_read as u32;
                    self.palette.truncate(original_palette_size + bytes_read);
                    if bytes_left_in_chunk > 0 {
                        self.decode_state = DecodeState::ReadingPalette(bytes_left_in_chunk);
                        continue
                    }

                    // Send the palette over to the predictor thread.
                    self.predictor_thread_comm
                        .sender
                        .send(MainThreadToPredictorThreadMsg::SetPalette(self.palette.clone()))
                        .unwrap();

                    // Move past the CRC.
                    try!(reader.seek(SeekFrom::Current(4)).map_err(PngError::Io));

                    // Start looking for the image data.
                    self.decode_state = DecodeState::LookingForImageData
                }
                DecodeState::DecodingData(bytes_left_in_chunk) => {
                    let (width, color_depth) = {
                        let metadata = self.metadata.as_ref().expect("No metadata?!");
                        (metadata.dimensions.width, metadata.color_depth)
                    };
                    let bytes_per_pixel = bytes_per_pixel(color_depth) as u32;
                    let stride = width * bytes_per_pixel * bytes_per_pixel /
                        InterlacingInfo::new(0, color_depth, self.current_lod).stride as u32;
                    if self.scanline_data_buffer_info.is_some() {
                        return Ok(AddDataResult::BufferFull)
                    }

                    let bytes_read;
                    if self.compressed_data_buffer.len() < BUFFER_SIZE {
                        let original_length = self.compressed_data_buffer.len();
                        let target_length =
                            cmp::min(BUFFER_SIZE, original_length + bytes_left_in_chunk as usize);
                        self.compressed_data_buffer.resize(target_length, 0);
                        bytes_read =
                            try!(reader.read(&mut self.compressed_data_buffer[original_length..])
                                       .map_err(PngError::Io));
                        debug_assert!(self.compressed_data_buffer.len() <= original_length +
                                      bytes_read);
                        self.compressed_data_buffer.truncate(original_length + bytes_read);
                    } else {
                        bytes_read = 0
                    }

                    unsafe {
                        self.z_stream.avail_in = (self.compressed_data_buffer.len() -
                                                  self.compressed_data_consumed) as u32;
                        self.z_stream.next_in =
                            &mut self.compressed_data_buffer[self.compressed_data_consumed];

                        // Read the scanline data.
                        //
                        // TODO(pcwalton): This may well show up in profiles. Probably we are going
                        // to want to read multiple scanlines at once. Before we do this, though,
                        // we are going to have to deal with SSE alignment restrictions.
                        if self.z_stream.avail_in != 0 {
                            // Make room for the stride + 32 bytes, which should be enough to
                            // handle any amount of padding on both ends.
                            self.scanline_data_buffer
                                .extend_with_uninitialized(1 + (stride as usize) + 32);
                            let offset = self.aligned_scanline_buffer_offset();
                            let original_size = self.scanline_data_buffer_size;
                            self.z_stream.avail_out = 1 + stride - (original_size as u32);
                            self.z_stream.next_out =
                                &mut self.scanline_data_buffer[offset + original_size - 1];
                            debug_assert!(self.z_stream.avail_out as usize + original_size +
                                          offset <= self.scanline_data_buffer.len());
                            try!(PngError::from_zlib_result(libz_sys::inflate(
                                    &mut self.z_stream,
                                    Z_NO_FLUSH)));
                            self.advance_compressed_data_offset();
                            self.scanline_data_buffer_size =
                                (1 + stride - self.z_stream.avail_out) as usize;
                        } else {
                            return Ok((AddDataResult::Continue))
                        }
                    }

                    // Advance the Y position if necessary.
                    if self.scanline_data_buffer_size == 1 + stride as usize {
                        self.scanline_data_buffer_info = Some(BufferedScanlineInfo {
                            lod: self.current_lod,
                            y: self.current_y,
                        });
                        self.current_y += 1;
                        let height = self.metadata
                                         .as_ref()
                                         .expect("No metadata?!")
                                         .dimensions
                                         .height;
                        let y_scale_factor = InterlacingInfo::y_scale_factor(self.current_lod);
                        if self.current_y == height / y_scale_factor {
                            self.current_y = 0;
                            if let LevelOfDetail::Adam7(ref mut current_lod) = self.current_lod {
                                *current_lod += 1
                            }
                        }
                    }

                    let bytes_left_in_chunk_after_read = bytes_left_in_chunk - bytes_read as u32;
                    self.decode_state = if bytes_left_in_chunk_after_read == 0 &&
                            self.compressed_data_consumed >= self.compressed_data_buffer.len() {
                        // Skip over the CRC.
                        try!(reader.seek(SeekFrom::Current(4)).map_err(PngError::Io));
                        DecodeState::LookingForImageData
                    } else {
                        DecodeState::DecodingData(bytes_left_in_chunk_after_read)
                    }
                }
                DecodeState::Finished => return Ok(AddDataResult::Finished),
            }
        }
    }

    pub fn advance_compressed_data_offset(&mut self) {
        self.compressed_data_consumed = self.compressed_data_buffer.len() -
            self.z_stream.avail_in as usize;
        if self.compressed_data_consumed == self.compressed_data_buffer.len() {
            self.compressed_data_consumed = 0;
            self.compressed_data_buffer.truncate(0)
        }
    }

    #[inline(never)]
    pub fn decode(&mut self) -> Result<(),PngError> {
        let (dimensions, color_depth, color_type) = match self.metadata {
            None => return Err(PngError::NoMetadata),
            Some(ref metadata) => (metadata.dimensions, metadata.color_depth, metadata.color_type),
        };
        let scanline_buffer_offset = self.aligned_scanline_buffer_offset();

        if let Some(scanline_info) = self.scanline_data_buffer_info {
            let predictor = self.scanline_data_buffer[scanline_buffer_offset - 1];

            let empty_scanline_data_buffer = match self.cached_scanline_data_buffers.pop() {
                None => vec![],
                Some(cached_scanline_data_buffer) => cached_scanline_data_buffer,
            };
            let msg = MainThreadToPredictorThreadMsg::Predict(PredictionRequest {
                width: dimensions.width,
                height: dimensions.height,
                color_depth: color_depth,
                indexed_color: color_type == ColorType::Indexed,
                predictor: try!(Predictor::from_byte(predictor)),
                scanline_data: mem::replace(&mut self.scanline_data_buffer,
                                            empty_scanline_data_buffer),
                scanline_offset: scanline_buffer_offset,
                scanline_lod: scanline_info.lod,
                scanline_y: scanline_info.y,
            });
            self.scanline_data_buffer_size = 0;
            self.predictor_thread_comm.sender.send(msg).unwrap();
            self.predictor_thread_comm.scanlines_in_progress += 1;
            self.scanline_data_buffer_info = None;
        }

        while let Ok(msg) = self.predictor_thread_comm.receiver.try_recv() {
            try!(self.handle_predictor_thread_msg(msg));
        }

        Ok(())
    }

    fn handle_predictor_thread_msg(&mut self, msg: PredictorThreadToMainThreadMsg)
                                   -> Result<(),PngError> {
        match msg {
            PredictorThreadToMainThreadMsg::NoDataProviderError => Err(PngError::NoDataProvider),
            PredictorThreadToMainThreadMsg::ScanlineComplete(y, lod, mut buffer) => {
                buffer.clear();
                self.cached_scanline_data_buffers.push(buffer);
                if lod > self.last_decoded_lod {
                    self.last_decoded_lod = lod;
                    self.scanlines_decoded_in_this_lod = 0;
                }
                if y >= self.scanlines_decoded_in_this_lod {
                    debug_assert!(self.last_decoded_lod == lod);
                    self.scanlines_decoded_in_this_lod = y + 1
                }
                Ok(())
            }
        }
    }

    #[inline(never)]
    pub fn wait_until_finished(&mut self) -> Result<(),PngError> {
        let height = self.metadata.as_ref().expect("No metadata yet!").dimensions.height;

        while (self.current_lod == LevelOfDetail::None ||
                   self.current_lod < LevelOfDetail::Adam7(6)) &&
                self.scanlines_decoded_in_this_lod < height {
            let msg = self.predictor_thread_comm
                          .receiver
                          .recv()
                          .expect("Predictor thread hung up!");
            try!(self.handle_predictor_thread_msg(msg));
        }
        Ok(())
    }

    #[inline(never)]
    pub fn set_data_provider(&mut self, data_provider: Box<DataProvider>) {
        self.predictor_thread_comm
            .sender
            .send(MainThreadToPredictorThreadMsg::SetDataProvider(data_provider))
            .unwrap()
    }

    #[inline(never)]
    pub fn extract_data(&mut self) {
        self.predictor_thread_comm
            .sender
            .send(MainThreadToPredictorThreadMsg::ExtractData)
            .unwrap()
    }

    #[inline]
    pub fn metadata(&self) -> &Option<Metadata> {
        &self.metadata
    }

    fn aligned_scanline_buffer_offset(&self) -> usize {
        let offset = aligned_offset_for_slice(&self.scanline_data_buffer[..]);
        if offset == 0 {
            16
        } else {
            offset
        }
    }
}

#[derive(Debug)]
pub enum PngError {
    NeedMoreData,
    Io(io::Error),
    InvalidMetadata(String),
    InvalidOperation(&'static str),
    InvalidData(String),
    EntropyDecodingError(c_int),
    InvalidScanlinePredictor(u8),
    NoMetadata,
    NoDataProvider,
    AlignmentError,
}

impl PngError {
    fn from_zlib_result(error: c_int) -> Result<(),PngError> {
        match error {
            Z_OK | Z_STREAM_END => Ok(()),
            Z_ERRNO => Err(PngError::Io(io::Error::last_os_error())),
            _ => Err(PngError::EntropyDecodingError(error)),
        }
    }
}

#[derive(Copy, Clone, PartialEq)]
pub enum AddDataResult {
    Continue,
    BufferFull,
    Finished,
}

#[derive(Copy, Clone, PartialEq)]
enum DecodeState {
    Start,
    LookingForImageData,
    LookingForPalette,
    ReadingPalette(u32),
    DecodingData(u32),
    Finished,
}

fn aligned_offset_for_slice(slice: &[u8]) -> usize {
    let address = slice.as_ptr() as usize;
    let remainder = address % 16;
    if remainder == 0 {
        0
    } else {
        16 - remainder
    }
}

#[allow(non_snake_case)]
unsafe fn inflateInit(strm: *mut z_stream) -> c_int {
    let version = libz_sys::zlibVersion();
    libz_sys::inflateInit_(strm, version, mem::size_of::<z_stream>() as c_int)
}

pub trait DataProvider : Send {
    /// `reference_scanline`, if present, will always be above `current_scanline`.
    fn get_scanline_data<'a>(&'a mut self,
                             reference_scanline: Option<u32>,
                             current_scanline: u32,
                             lod: LevelOfDetail)
                             -> ScanlineData;
    fn extract_data(&mut self);
}

pub struct ScanlineData<'a> {
    pub reference_scanline: Option<&'a mut [u8]>,
    pub current_scanline: &'a mut [u8],
    pub stride: u8,
}

pub trait UninitializedExtension {
    unsafe fn extend_with_uninitialized(&mut self, new_len: usize);
}

impl UninitializedExtension for Vec<u8> {
    unsafe fn extend_with_uninitialized(&mut self, new_len: usize) {
        if self.len() >= new_len {
            return
        }
        self.reserve(new_len);
        self.set_len(new_len);
    }
}

#[inline]
pub fn align(address: usize) -> usize {
    let remainder = address % 16;
    if remainder == 0 {
        address
    } else {
        address + 16 - remainder
    }
}

#[inline]
pub fn bytes_per_pixel(color_depth: u8) -> u8 {
    cmp::max(color_depth / 8, 1)
}

#[derive(Copy, Clone, Debug)]
pub struct InterlacingInfo {
    pub y: u32,
    pub stride: u8,
    pub offset: u8,
}

impl InterlacingInfo {
    pub fn new(y: u32, color_depth: u8, lod: LevelOfDetail) -> InterlacingInfo {
        let y_scale_factor = InterlacingInfo::y_scale_factor(lod);
        let bytes_per_pixel = bytes_per_pixel(color_depth);
        let (y_offset, stride, x_offset) = match lod {
            LevelOfDetail::None => (0, 1, 0),
            LevelOfDetail::Adam7(0) => (0, 8, 0),
            LevelOfDetail::Adam7(1) => (0, 8, 4),
            LevelOfDetail::Adam7(2) => (4, 4, 0),
            LevelOfDetail::Adam7(3) => (0, 4, 2),
            LevelOfDetail::Adam7(4) => (2, 2, 0),
            LevelOfDetail::Adam7(5) => (0, 2, 1),
            LevelOfDetail::Adam7(6) => (1, 1, 0),
            LevelOfDetail::Adam7(_) => panic!("Unsupported Adam7 level of detail!"),
        };
        InterlacingInfo {
            y: y * y_scale_factor + y_offset,
            stride: stride * bytes_per_pixel,
            offset: x_offset * bytes_per_pixel,
        }
    }

    fn y_scale_factor(lod: LevelOfDetail) -> u32 {
        match lod {
            LevelOfDetail::None => 1,
            LevelOfDetail::Adam7(0) | LevelOfDetail::Adam7(1) | LevelOfDetail::Adam7(2) => 8,
            LevelOfDetail::Adam7(3) | LevelOfDetail::Adam7(4) => 4,
            LevelOfDetail::Adam7(5) | LevelOfDetail::Adam7(6) => 2,
            LevelOfDetail::Adam7(_) => panic!("Unsupported Adam7 level of detail!"),
        }
    }
}

#[derive(Copy, Clone)]
struct BufferedScanlineInfo {
    y: u32,
    lod: LevelOfDetail,
}

#[derive(Copy, Clone, PartialEq, PartialOrd, Debug)]
pub enum LevelOfDetail {
    None,
    Adam7(u8),
}

