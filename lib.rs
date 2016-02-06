//! A parallel PNG decoder.

extern crate byteorder;
extern crate libc;
extern crate num;
extern crate rayon;
extern crate zlib_ng_sys;

use libc::c_int;
use metadata::{ChunkHeader, Metadata};
use std::io::{self, BufRead, Seek, SeekFrom};
use std::mem;
use zlib_ng_sys::{Z_ERRNO, Z_NO_FLUSH, Z_OK, Z_STREAM_END, z_stream};

const BUFFER_SIZE: u32 = 16384;
const ESTIMATED_COMPRESSED_DATA_CHUNK_SIZE: usize = 1024 * 1024;
const MIN_SCANLINES_IN_SEQUENTIAL_GROUP: u32 = 32;

pub mod metadata;

pub struct Image {
    metadata: Option<Metadata>,
    z_stream: z_stream,
    buffered_compressed_data: Vec<u8>,
    buffered_uncompressed_data: Vec<u8>,
    buffered_scanline_predictors: Vec<Predictor>,
    buffered_scanline_data: Vec<u8>,
    decode_state: DecodeState,
    zero_scanline: Vec<u8>,
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
            buffered_uncompressed_data: vec![],
            buffered_scanline_predictors: vec![],
            buffered_scanline_data: vec![],
            decode_state: DecodeState::Start,
            zero_scanline: vec![],
        })
    }

    #[inline(never)]
    pub fn preallocate_space(&mut self,
                             estimated_width: u32,
                             estimated_height: u32,
                             estimated_bpp: u32) {
        self.buffered_compressed_data.reserve(ESTIMATED_COMPRESSED_DATA_CHUNK_SIZE);
        self.buffered_uncompressed_data
            .reserve((estimated_width * estimated_height * estimated_bpp + estimated_height) as
                     usize);
        self.buffered_scanline_predictors.reserve(estimated_height as usize);
        self.buffered_scanline_data
            .reserve((estimated_width * estimated_height * estimated_bpp) as usize);
    }

    #[inline(never)]
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
                            let original_buffer_length = self.buffered_uncompressed_data.len();
                            self.buffered_uncompressed_data.reserve(BUFFER_SIZE as usize);
                            self.z_stream.avail_out = BUFFER_SIZE;
                            self.z_stream.next_out =
                                    self.buffered_uncompressed_data
                                        .as_mut_ptr()
                                        .offset(original_buffer_length as isize);
                            try!(PngError::from_zlib_result(zlib_ng_sys::inflate(
                                        &mut self.z_stream,
                                        Z_NO_FLUSH)));
                            self.buffered_uncompressed_data
                                .set_len((original_buffer_length + BUFFER_SIZE as usize -
                                          self.z_stream.avail_out as usize));
                        }
                    }


                    let stride = self.stride() as usize;
                    for uncompressed_data in self.buffered_uncompressed_data.chunks(stride + 1) {
                        if uncompressed_data.len() == stride + 1 {
                            self.buffered_scanline_predictors
                                .push(try!(Predictor::from_byte(uncompressed_data[0])));
                            self.buffered_scanline_data.extend_from_slice(&uncompressed_data[1..])
                        }
                    }

                    let original_buffered_uncompressed_data_len =
                        self.buffered_uncompressed_data.len();
                    if original_buffered_uncompressed_data_len > stride + 1 {
                        let leftover_data_length = original_buffered_uncompressed_data_len %
                            (stride + 1);
                        {
                            let (head, tail) =
                                self.buffered_uncompressed_data
                                    .split_at_mut(original_buffered_uncompressed_data_len -
                                                  leftover_data_length);
                            head[0..leftover_data_length].clone_from_slice(tail);
                        }
                        self.buffered_uncompressed_data.truncate(leftover_data_length);
                    }

                    let bytes_left_in_chunk_after_read = bytes_left_in_chunk - byte_count;
                    self.decode_state = if bytes_left_in_chunk_after_read == 0 {
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

    #[inline(never)]
    pub fn decode(&mut self, result: &mut Vec<u8>) {
        let metadata = self.metadata.as_ref().unwrap();
        let image_width = metadata.dimensions.width;
        let color_depth = metadata.color_depth;
        let stride = self.stride();
        println!("predictor count={} buffered_scanline_data count={}",
                 self.buffered_scanline_predictors.len(),
                 self.buffered_scanline_data.len());
        let total_scanline_count = (self.buffered_scanline_data.len() / (stride as usize)) as u32;
        let output_size = (stride * total_scanline_count) as usize;
        let old_result_size = result.len();

        // Make room in `result`. If `result` is already empty, then avoid the copy.
        if result.is_empty() {
            mem::swap(result, &mut self.buffered_scanline_data);
            result.truncate(output_size);
        } else {
            result.extend_from_slice(&self.buffered_scanline_data[0..output_size]);
            self.buffered_scanline_data = self.buffered_scanline_data[output_size..].to_vec();
        }

        if self.zero_scanline.len() < stride as usize {
            self.zero_scanline.resize(stride as usize, 0)
        }

        decode_job(image_width,
                   color_depth,
                   &self.zero_scanline[..],
                   &self.buffered_scanline_predictors[..],
                   &mut result[old_result_size..]);
        return;

        fn decode_job(width: u32,
                      color_depth: u8,
                      zero_scanline: &[u8],
                      predictors: &[Predictor],
                      data: &mut [u8]) {
            if predictors.is_empty() {
                return
            }

            let mut scanlines_in_sequential_group: u32 = 1;
            while (scanlines_in_sequential_group as usize) < predictors.len() {
                match predictors[scanlines_in_sequential_group as usize] {
                    Predictor::None | Predictor::Left => {
                        if scanlines_in_sequential_group >= MIN_SCANLINES_IN_SEQUENTIAL_GROUP {
                            break
                        }
                    }
                    Predictor::Up | Predictor::Average | Predictor::Paeth => {}
                }
                scanlines_in_sequential_group += 1
            }

            let stride = width * (color_depth / 8) as u32;
            let sequential_group_byte_length = (scanlines_in_sequential_group as usize) *
                (width as usize) * ((color_depth / 8) as usize);
            let (head_data, tail_data) = data.split_at_mut(sequential_group_byte_length);
            rayon::join(|| {
                let mut data_iterator = head_data.chunks_mut(stride as usize);
                let mut prev = &zero_scanline[..];
                //println!("--- start sequential group ---");
                for scanline in 0..scanlines_in_sequential_group {
                    //println!("{:?}", predictors[scanline as usize]);
                    let decode_scanline = match predictors[scanline as usize] {
                        Predictor::None => parng_predict_scanline_none,
                        Predictor::Left => parng_predict_scanline_left,
                        Predictor::Up => parng_predict_scanline_up,
                        Predictor::Average => parng_predict_scanline_average,
                        Predictor::Paeth => parng_predict_scanline_paeth,
                    };
                    let mut head = data_iterator.next().expect("Unexpected end of data!");
                    unsafe {
                        decode_scanline(&mut head[0], &prev[0], width as u64)
                    }
                    prev = head
                }
                //println!("--- end sequential group ---");
            }, || {
                decode_job(width,
                           color_depth,
                           zero_scanline,
                           &predictors[(scanlines_in_sequential_group as usize)..],
                           tail_data)
            });
        }
    }

    #[inline]
    pub fn metadata(&self) -> &Option<Metadata> {
        &self.metadata
    }

    fn stride(&self) -> u32 {
        let metadata = self.metadata.as_ref().unwrap();
        let image_width = metadata.dimensions.width;
        let color_depth = metadata.color_depth;
        image_width * ((color_depth / 8) as u32)
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
    Finished,
}

#[derive(Copy, Clone, PartialEq)]
enum DecodeState {
    Start,
    LookingForImageData,
    DecodingData(u32),
    Finished,
}

#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(u8)]
enum Predictor {
    None = 0,
    Left = 1,
    Up = 2,
    Average = 3,
    Paeth = 4,
}

impl Predictor {
    fn from_byte(byte: u8) -> Result<Predictor,PngError> {
        match byte {
            0 => Ok(Predictor::None),
            1 => Ok(Predictor::Left),
            2 => Ok(Predictor::Up),
            3 => Ok(Predictor::Average),
            4 => Ok(Predictor::Paeth),
            byte => Err(PngError::InvalidScanlinePredictor(byte)),
        }
    }
}

#[link(name="parngpredict")]
extern {
    fn parng_predict_scanline_none(this: *mut u8, prev: *const u8, width: u64);
    fn parng_predict_scanline_left(this: *mut u8, prev: *const u8, width: u64);
    fn parng_predict_scanline_up(this: *mut u8, prev: *const u8, width: u64);
    fn parng_predict_scanline_average(this: *mut u8, prev: *const u8, width: u64);
    fn parng_predict_scanline_paeth(this: *mut u8, prev: *const u8, width: u64);
}

