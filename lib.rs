//! A parallel PNG decoder.

extern crate byteorder;
extern crate libc;
extern crate num;
extern crate rayon;
extern crate zlib_ng_sys;

use libc::c_int;
use metadata::{ChunkHeader, Metadata};
use std::cmp;
use std::io::{self, BufRead, Seek, SeekFrom};
use zlib_ng_sys::{Z_ERRNO, Z_NO_FLUSH, Z_OK, z_stream};

const BUFFER_SIZE: u32 = 16384;

pub mod metadata;

pub struct Image<R> {
    reader: R,
    metadata: Option<Metadata>,
    z_stream: Option<z_stream>,
    buffered_compressed_data: Vec<u8>,
    buffered_scanline_predictors: Vec<Predictor>,
    buffered_scanline_data: Vec<u8>,
    decode_state: DecodeState,
}

impl<R> Drop for Image<R> {
    fn drop(&mut self) {
        unsafe {
            if let Some(ref mut z_stream) = self.z_stream {
                drop(zlib_ng_sys::deflateEnd(z_stream))
            }
        }
    }
}

impl<R> Image<R> where R: BufRead + Seek {
    pub fn new(reader: R) -> Image<R> {
        Image {
            reader: reader,
            metadata: None,
            z_stream: None,
            buffered_compressed_data: vec![],
            buffered_scanline_predictors: vec![],
            buffered_scanline_data: vec![],
            decode_state: DecodeState::Start,
        }
    }

    pub fn load_metadata(&mut self) -> Result<(),PngError> {
        if self.decode_state != DecodeState::Start {
            return Err(PngError::InvalidOperation("Metadata has already been loaded!"))
        }
        self.metadata = Some(try!(Metadata::load(&mut self.reader)));
        self.decode_state = DecodeState::MetadataLoaded;
        Ok(())
    }

    pub fn start_decoding(&mut self) -> Result<(),PngError> {
        match self.decode_state {
            DecodeState::Start => {
                return Err(PngError::InvalidOperation("Call `load_metadata()` before \
                                                      `start_decoding()`!"))
            }
            DecodeState::DecodingData(_) => {
                return Err(PngError::InvalidOperation("Decoding has already begun!"))
            }
            DecodeState::MetadataLoaded => {}
        }

        let mut chunk_length;
        loop {
            let chunk_header = try!(ChunkHeader::load(&mut self.reader));
            chunk_length = chunk_header.length;
            if &chunk_header.chunk_type == b"IDAT" {
                break
            }

            // Add 4 to skip over the CRC of this chunk.
            //
            // FIXME(pcwalton): We should have some "need more data" error return mechanism.
            try!(self.reader
                     .seek(SeekFrom::Current((chunk_header.length as i64) + 4))
                     .map_err(PngError::Io));
        }

        unsafe {
            debug_assert!(self.z_stream.is_none());
            self.z_stream = Some(z_stream::default());
            let mut z_stream = self.z_stream.as_mut().unwrap();
            try!(PngError::from_zlib_result(zlib_ng_sys::inflateInit(z_stream)));
        }

        self.decode_state = DecodeState::DecodingData(chunk_length);
        Ok(())
    }

    pub fn buffer_data(&mut self, mut byte_count: u32) -> Result<(),PngError> {
        let bytes_left_in_chunk = match self.decode_state {
            DecodeState::Start | DecodeState::MetadataLoaded => {
                return Err(PngError::InvalidOperation("Call `load_metadata()` and \
                                                       `start_decoding()` before \
                                                       `buffer_data()`!"))
            }
            DecodeState::DecodingData(bytes_left_in_chunk) => bytes_left_in_chunk,
        };
        byte_count = cmp::min(byte_count, bytes_left_in_chunk);

        if self.buffered_compressed_data.len() < (byte_count as usize) {
            self.buffered_compressed_data.resize(byte_count as usize, 0)
        }
        try!(self.reader
                 .read_exact(&mut self.buffered_compressed_data[0..(byte_count as usize)])
                 .map_err(PngError::Io));

        unsafe {
            debug_assert!(self.z_stream.is_some());
            let mut z_stream = self.z_stream.as_mut().unwrap();
            z_stream.avail_in = byte_count as u32;
            z_stream.next_in = &self.buffered_compressed_data[0];
            while z_stream.avail_in != 0 {
                let old_buffered_scanline_data_length = self.buffered_scanline_data.len();
                self.buffered_scanline_data
                    .resize(old_buffered_scanline_data_length + (BUFFER_SIZE as usize), 0);
                z_stream.avail_out = BUFFER_SIZE;
                z_stream.next_out =
                    &mut self.buffered_scanline_data[old_buffered_scanline_data_length] as *mut u8;
                try!(PngError::from_zlib_result(zlib_ng_sys::inflate(z_stream, Z_NO_FLUSH)));
            }
        }

        self.decode_state = DecodeState::DecodingData(bytes_left_in_chunk - byte_count);
        Ok(())
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

            let mut scanlines_in_sequential_group: u32 = 0;
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
                    let predictor = predictors[scanline as usize];
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
pub enum DecodeState {
    Start,
    MetadataLoaded,
    DecodingData(u32),
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

