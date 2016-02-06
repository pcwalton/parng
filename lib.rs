//! A parallel PNG decoder.

extern crate byteorder;
extern crate libc;
extern crate num;
extern crate rayon;
extern crate zlib_ng_sys;

use libc::c_int;
use metadata::{ChunkHeader, Metadata};
use std::io::{self, BufRead, Seek, SeekFrom};
use zlib_ng_sys::{Z_ERRNO, Z_NO_FLUSH, Z_OK, z_stream};

const BUFFER_SIZE: u32 = 16384;

pub mod metadata;

pub struct Image {
    metadata: Option<Metadata>,
    z_stream: z_stream,
    buffered_compressed_data: Vec<u8>,
    buffered_scanline_predictors: Vec<Predictor>,
    buffered_scanline_data: Vec<u8>,
    decode_state: DecodeState,
}

impl Drop for Image {
    fn drop(&mut self) {
        unsafe {
            drop(zlib_ng_sys::deflateEnd(&mut self.z_stream))
        }
    }
}

impl Image {
    pub fn new() -> Result<Image,PngError> {
        let mut z_stream = z_stream::default();
        unsafe {
            try!(PngError::from_zlib_result(zlib_ng_sys::inflateInit(&mut z_stream)));
        }
        Ok(Image {
            metadata: None,
            z_stream: z_stream,
            buffered_compressed_data: vec![],
            buffered_scanline_predictors: vec![],
            buffered_scanline_data: vec![],
            decode_state: DecodeState::Start,
        })
    }

    pub fn add_data<R>(&mut self, reader: &mut R) -> Result<AddDataResult,PngError>
                       where R: BufRead + Seek {
        loop {
            let initial_pos = try!(reader.seek(SeekFrom::Current(0)).map_err(PngError::Io));
            match self.decode_state {
                DecodeState::Start => {
                    match Metadata::load(reader) {
                        Ok(metadata) => {
                            self.metadata = Some(metadata);
                            self.decode_state = DecodeState::LookingForImageData
                        }
                        Err(PngError::NeedMoreData) => {
                            try!(reader.seek(SeekFrom::Start(initial_pos)).map_err(PngError::Io));
                            return Ok(AddDataResult::Continue)
                        }
                        Err(error) => return Err(error),
                    }
                }
                DecodeState::LookingForImageData => {
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
                DecodeState::DecodingData(bytes_left_in_chunk) => {
                    let mut byte_count = bytes_left_in_chunk;
                    if self.buffered_compressed_data.len() < (byte_count as usize) {
                        self.buffered_compressed_data.resize(byte_count as usize, 0)
                    }

                    {
                        let mut buffered_compressed_data =
                            &mut self.buffered_compressed_data[0..(byte_count as usize)];
                        match reader.read(&mut buffered_compressed_data) {
                            Ok(0) => {
                                try!(reader.seek(SeekFrom::Start(initial_pos))
                                           .map_err(PngError::Io));
                                return Ok(AddDataResult::Continue)
                            }
                            Ok(bytes_read) => byte_count = bytes_read as u32,
                            Err(error) => return Err(PngError::Io(error)),
                        }
                    }

                    unsafe {
                        self.z_stream.avail_in = byte_count as u32;
                        self.z_stream.next_in = &self.buffered_compressed_data[0];
                        while self.z_stream.avail_in != 0 {
                            let old_buffered_scanline_data_length =
                                self.buffered_scanline_data.len();
                            self.buffered_scanline_data.resize(
                                old_buffered_scanline_data_length + (BUFFER_SIZE as usize), 0);
                            self.z_stream.avail_out = BUFFER_SIZE;
                            self.z_stream.next_out = &mut self.buffered_scanline_data[
                                old_buffered_scanline_data_length] as *mut u8;
                            try!(PngError::from_zlib_result(zlib_ng_sys::inflate(
                                        &mut self.z_stream,
                                        Z_NO_FLUSH)));
                        }
                    }

                    let bytes_left_in_chunk_after_read = bytes_left_in_chunk - byte_count;
                    self.decode_state = if bytes_left_in_chunk_after_read == 0 {
                        DecodeState::LookingForImageData
                    } else {
                        DecodeState::DecodingData(bytes_left_in_chunk_after_read)
                    }
                }
                DecodeState::Finished => return Ok(AddDataResult::Finished),
            }
        }
    }

    pub fn decode(&mut self, result: &mut Vec<u8>) {
        let metadata = self.metadata.as_ref().unwrap();
        let image_width = metadata.dimensions.width;
        let color_depth = metadata.color_depth;
        let total_scanline_count = (self.buffered_scanline_data.len() / (image_width as usize)) as
            u32;
        let total_size = (image_width * total_scanline_count) as usize;
        if result.len() < total_size {
            result.resize(total_size, 0)
        }

        decode_job(image_width,
                   color_depth,
                   &self.buffered_scanline_predictors[..],
                   &self.buffered_scanline_data[0..total_size],
                   &mut result[0..total_size]);
        return;

        fn decode_job(width: u32,
                      color_depth: u8,
                      predictors: &[Predictor],
                      input: &[u8],
                      output: &mut [u8]) {
            if predictors.is_empty() {
                return
            }

            let mut scanlines_in_sequential_group: u32 = 1;
            while (scanlines_in_sequential_group as usize) < predictors.len() {
                match predictors[scanlines_in_sequential_group as usize] {
                    Predictor::None | Predictor::Left => break,
                    Predictor::Up | Predictor::Average | Predictor::Paeth => {}
                }
                scanlines_in_sequential_group += 1
            }

            let sequential_group_byte_length = (scanlines_in_sequential_group as usize) *
                (width as usize) * (color_depth as usize);
            let (head_output, tail_output) = output.split_at_mut(sequential_group_byte_length);
            rayon::join(|| {
                for scanline in 0..scanlines_in_sequential_group {
                    let decode_scanline = match predictors[scanline] {
                        Predictor::None => parng_predict_scanline_none,
                        Predictor::Left => parng_predict_scanline_left,
                        Predictor::Up => parng_predict_scanline_up,
                        Predictor::Average => parng_predict_scanline_average,
                        Predictor::Paeth => parng_predict_scanline_paeth,
                    }
                    unsafe {
                    }
                    // TODO(pcwalton): Predict!
                }
            }, || {
                decode_job(width,
                           color_depth,
                           &predictors[(scanlines_in_sequential_group as usize)..],
                           &input[sequential_group_byte_length..],
                           tail_output)
            });
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
}

impl PngError {
    fn from_zlib_result(error: c_int) -> Result<(),PngError> {
        match error {
            Z_OK => Ok(()),
            Z_ERRNO => Err(PngError::Io(io::Error::last_os_error())),
            _ => Err(PngError::EntropyDecodingError(error)),
        }
    }
}

#[derive(Copy, Clone, PartialEq)]
pub enum AddDataResult {
    Continue,
    Finished,
}

#[derive(Copy, Clone, PartialEq)]
enum DecodeState {
    Start,
    LookingForImageData,
    DecodingData(u32),
    Finished,
}

#[derive(Copy, Clone, PartialEq)]
#[repr(u8)]
enum Predictor {
    None = 0,
    Left = 1,
    Up = 2,
    Average = 3,
    Paeth = 4,
}

#[link(name="parngpredict")]
extern {
    fn parng_predict_scanline_none(this: *mut u8, prev: *const u8, width: u64);
    fn parng_predict_scanline_left(this: *mut u8, prev: *const u8, width: u64);
    fn parng_predict_scanline_up(this: *mut u8, prev: *const u8, width: u64);
    fn parng_predict_scanline_average(this: *mut u8, prev: *const u8, width: u64);
    fn parng_predict_scanline_paeth(this: *mut u8, prev: *const u8, width: u64);
}

