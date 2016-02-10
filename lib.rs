//! A parallel PNG decoder.

extern crate byteorder;
extern crate libc;
extern crate libz_sys;
extern crate num;

use interlacing::{LevelOfDetail, LodDimensionQuery};
use libc::c_int;
use libz_sys::{Z_ERRNO, Z_NO_FLUSH, Z_OK, Z_STREAM_END, z_stream};
use metadata::{ChunkHeader, InterlaceMethod, Metadata};
use std::cmp;
use std::io::{self, Read, Seek, SeekFrom};
use std::mem;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

const BUFFER_SIZE: usize = 16384;

pub mod interlacing;
pub mod metadata;

pub struct Image {
    metadata: Option<Metadata>,
    z_stream: z_stream,
    compressed_data_buffer: Vec<u8>,
    compressed_data_consumed: usize,
    scanline_data_buffer: Vec<u8>,
    scanline_data_buffer_size: usize,

    /// If `None`, the buffered scanline isn't full yet.
    ///
    /// FIXME(pcwalton): This is kind of an ugly way to keep track of the scanline data buffer's
    /// fullness; can we do something nicer?
    scanline_data_buffer_lod: Option<LevelOfDetail>,

    current_y: u32,
    current_lod: LevelOfDetail,
    decode_state: DecodeState,
    predictor_thread_comm: Option<MainThreadToPredictorThreadComm>,
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
            scanline_data_buffer: vec![],
            scanline_data_buffer_size: 0,
            scanline_data_buffer_lod: None,
            current_y: 0,
            current_lod: LevelOfDetail::None,
            decode_state: DecodeState::Start,
            predictor_thread_comm: None,
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
                DecodeState::DecodingData(bytes_left_in_chunk) => {
                    let stride = self.stride_for_lod(self.current_lod);
                    if self.scanline_data_buffer_lod.is_some() {
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
                            self.scanline_data_buffer.resize(1 + (stride as usize) + 32, 0);
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
                        self.current_y += 1;
                        self.scanline_data_buffer_lod = Some(self.current_lod);
                        //println!("incrementing current Y: now at {}", self.current_y);
                        if self.current_y == self.height_for_lod(self.current_lod) {
                            self.current_y = 0;
                            if let LevelOfDetail::Adam7(ref mut current_lod) = self.current_lod {
                                *current_lod += 1;
                                println!("incrementing LOD: now at {}", *current_lod);
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
                DecodeState::Finished => {
                    //println!("Finished!");
                    return Ok(AddDataResult::Finished)
                }
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
    pub fn decode(&mut self) -> Result<DecodeResult,PngError> {
        let width_and_lod_for_scanline_data_buffer_if_full =
            self.scanline_data_buffer_lod.map(|lod| (self.width_for_lod(lod), lod));
        let scanline_buffer_offset = self.aligned_scanline_buffer_offset();

        let result;
        {
            if self.predictor_thread_comm.is_none() {
                self.predictor_thread_comm = Some(MainThreadToPredictorThreadComm::new())
            }
            let predictor_thread_comm = self.predictor_thread_comm.as_mut().unwrap();

            result = if predictor_thread_comm.scanlines_in_progress > 0 {
                predictor_thread_comm.receiver.try_recv().ok().map(|msg| {
                    predictor_thread_comm.scanlines_in_progress -= 1;
                    (msg.0, msg.1)
                })
            } else {
                None
            };

            if let Some((scanline_width, scanline_lod)) =
                    width_and_lod_for_scanline_data_buffer_if_full {
                let predictor = self.scanline_data_buffer[scanline_buffer_offset - 1];
                //println!("predictor={}", predictor);

                let msg = MainThreadToPredictorThreadMsg::Predict(
                    scanline_width,
                    self.metadata.as_ref().unwrap().color_depth,
                    try!(Predictor::from_byte(predictor)),
                    mem::replace(&mut self.scanline_data_buffer, vec![]),
                    scanline_buffer_offset,
                    scanline_lod);
                self.scanline_data_buffer_size = 0;
                predictor_thread_comm.sender.send(msg).unwrap();
                predictor_thread_comm.scanlines_in_progress += 1
            }
            self.scanline_data_buffer_lod = None;
        }

        match result {
            Some((result_buffer, result_lod)) => {
                self.scanline_data_buffer = result_buffer;
                let stride = self.stride_for_lod(result_lod);
                let offset = self.aligned_scanline_buffer_offset();
                Ok(DecodeResult::Scanline(
                        &mut self.scanline_data_buffer[offset..(offset + stride as usize)],
                        self.current_lod))
            }
            None => Ok(DecodeResult::None),
        }
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

    fn predict(self, dest: &mut [u8], src: &[u8], prev: &[u8], width: u32, color_depth: u8) {
        let color_depth = color_depth as usize;
        let mut a: [u8; 4] = [0; 4];
        let mut c: [u8; 4] = [0; 4];
        match self {
            Predictor::None => {
                for (dest, src) in dest.chunks_mut(4).zip(src.chunks(color_depth)) {
                    for (dest, src) in dest.iter_mut().zip(src.iter()) {
                        *dest = *src
                    }
                }
            }
            Predictor::Left => {
                for (dest, src) in dest.chunks_mut(4).zip(src.chunks(color_depth)) {
                    for (dest, (src, a)) in dest.iter_mut().zip(src.iter().zip(a.iter_mut())) {
                        *a = src.wrapping_add(*a);
                        *dest = *a
                    }
                }
            }
            Predictor::Up => {
                for (dest, (src, b)) in dest.chunks_mut(4).zip(src.chunks(color_depth)
                                                                  .zip(prev.chunks(4))) {
                    for (dest, (src, b)) in dest.iter_mut().zip(src.iter().zip(b.iter())) {
                        *dest = src.wrapping_add(*b)
                    }
                }
            }
            Predictor::Average => {
                for (dest, (src, b)) in dest.chunks_mut(4).zip(src.chunks(color_depth)
                                                                  .zip(prev.chunks(4))) {
                    for (dest, (src, (b, a))) in
                            dest.iter_mut().zip(src.iter().zip(b.iter().zip(a.iter_mut()))) {
                        *a = src.wrapping_add((((*a as u16) + (*b as u16)) / 2) as u8);
                        *dest = *a
                    }
                }
            }
            Predictor::Paeth => {
                for (dest, (src, b)) in dest.chunks_mut(4).zip(src.chunks(color_depth)
                                                                  .zip(prev.chunks(4))) {
                    for (a, (b, (c, (dest, src)))) in
                        a.iter_mut().zip(b.iter().zip(c.iter_mut()
                                                       .zip(dest.iter_mut().zip(src.iter())))) {
                        let paeth = paeth(*a, *b, *c);
                        *a = src.wrapping_add(paeth);
                        *c = *b;
                        *dest = *a;
                    }
                }
            }
        }

        fn paeth(a: u8, b: u8, c: u8) -> u8 {
            let (a, b, c) = (a as i16, b as i16, c as i16);
            let p = a + b - c;
            let pa = (p - a).abs();
            let pb = (p - b).abs();
            let pc = (p - c).abs();
            if pa <= pb && pa <= pc {
                a as u8
            } else if pb <= pc {
                b as u8
            } else {
                c as u8
            }
        }
    }

    fn accelerated_predict(self,
                           dest: &mut [u8],
                           src: &[u8],
                           prev: &[u8],
                           width: u32,
                           color_depth: u8) {
        debug_assert!(((dest.as_ptr() as usize) & 0xf) == 0);
        debug_assert!(((src.as_ptr() as usize) & 0xf) == 0);
        debug_assert!(((prev.as_ptr() as usize) & 0xf) == 0);
        debug_assert!(color_depth == 32 || color_depth == 24);

        let decode_scanline = match (self, color_depth) {
            (Predictor::None, 32) => return,
            (Predictor::None, _) => {
                // FIXME(pcwalton): Implement this!
                panic!("None predictor only supported with 32 BPP!")
            }
            (Predictor::Left, 32) => parng_predict_scanline_left_32bpp,
            (Predictor::Left, 24) => parng_predict_scanline_left_24bpp,
            (Predictor::Up, 32) => parng_predict_scanline_up_32bpp,
            (Predictor::Up, 24) => parng_predict_scanline_up_24bpp,
            (Predictor::Average, 32) => parng_predict_scanline_average_32bpp,
            (Predictor::Average, 24) => parng_predict_scanline_average_24bpp,
            (Predictor::Paeth, 32) => parng_predict_scanline_paeth_32bpp,
            (Predictor::Paeth, 24) => parng_predict_scanline_paeth_24bpp,
            _ => panic!("Unsupported predictor/color depth combination!"),
        };
        unsafe {
            decode_scanline(dest.as_mut_ptr(), src.as_ptr(), prev.as_ptr(), width as u64)
        }
    }
}

enum MainThreadToPredictorThreadMsg {
    // Width, color depth, predictor, scanline data, scanline offset, and level of detail.
    Predict(u32, u8, Predictor, Vec<u8>, usize, LevelOfDetail),
}

struct PredictorThreadToMainThreadMsg(Vec<u8>, LevelOfDetail);

struct MainThreadToPredictorThreadComm {
    sender: Sender<MainThreadToPredictorThreadMsg>,
    receiver: Receiver<PredictorThreadToMainThreadMsg>,
    scanlines_in_progress: u32,
}

impl MainThreadToPredictorThreadComm {
    fn new() -> MainThreadToPredictorThreadComm {
        let (main_thread_to_predictor_thread_sender, main_thread_to_predictor_thread_receiver) =
            mpsc::channel();
        let (predictor_thread_to_main_thread_sender, predictor_thread_to_main_thread_receiver) =
            mpsc::channel();
        thread::spawn(move || {
            predictor_thread(predictor_thread_to_main_thread_sender,
                             main_thread_to_predictor_thread_receiver)
        });
        MainThreadToPredictorThreadComm {
            sender: main_thread_to_predictor_thread_sender,
            receiver: predictor_thread_to_main_thread_receiver,
            scanlines_in_progress: 0,
        }
    }
}

#[derive(Debug)]
pub enum DecodeResult<'a> {
    None,
    Scanline(&'a mut [u8], LevelOfDetail),
}

fn predictor_thread(sender: Sender<PredictorThreadToMainThreadMsg>,
                    receiver: Receiver<MainThreadToPredictorThreadMsg>) {
    let mut prev = vec![];
    let mut dest = vec![];
    while let Ok(msg) = receiver.recv() {
        match msg {
            MainThreadToPredictorThreadMsg::Predict(width,
                                                    color_depth,
                                                    predictor,
                                                    mut scanline,
                                                    scanline_offset,
                                                    scanline_lod) => {
                //println!("Predict(scanline_offset={})", scanline_offset);
                let stride = (width as usize) * 4;

                // Pad out to 32 bytes. This should be enough to handle any amount of padding at
                // both the beginning and end.
                while prev.len() < stride + 32 {
                    prev.push(0)
                }
                let prev_offset = aligned_offset_for_slice(&prev[..]);

                while dest.len() < stride + 32 {
                    dest.push(0)
                }
                let dest_offset = aligned_offset_for_slice(&dest[..]);

                match predictor {
                    Predictor::None | Predictor::Left | Predictor::Up | Predictor::Paeth |
                    Predictor::Average => {
                        predictor.accelerated_predict(&mut dest[dest_offset..],
                                                      &scanline[scanline_offset..],
                                                      &prev[prev_offset..],
                                                      width,
                                                      color_depth)
                    }
                }
                // FIXME(pcwalton): Any way to avoid this copy?
                prev[prev_offset..(prev_offset + stride)].clone_from_slice(
                    &mut dest[dest_offset..(dest_offset + stride)]);
                sender.send(PredictorThreadToMainThreadMsg(dest, scanline_lod)).unwrap();
                dest = scanline
            }
        }
    }
}

#[allow(non_snake_case)]
unsafe fn inflateInit(strm: *mut z_stream) -> c_int {
    let version = libz_sys::zlibVersion();
    libz_sys::inflateInit_(strm, version, mem::size_of::<z_stream>() as c_int)
}

#[link(name="parngacceleration")]
extern {
    fn parng_predict_scanline_left_32bpp(dest: *mut u8,
                                         src: *const u8,
                                         prev: *const u8,
                                         width: u64);
    fn parng_predict_scanline_left_24bpp(dest: *mut u8,
                                         src: *const u8,
                                         prev: *const u8,
                                         width: u64);
    fn parng_predict_scanline_up_32bpp(dest: *mut u8, src: *const u8, prev: *const u8, width: u64);
    fn parng_predict_scanline_up_24bpp(dest: *mut u8, src: *const u8, prev: *const u8, width: u64);
    fn parng_predict_scanline_average_32bpp(dest: *mut u8,
                                            src: *const u8,
                                            prev: *const u8,
                                            width: u64);
    fn parng_predict_scanline_average_24bpp(dest: *mut u8,
                                            src: *const u8,
                                            prev: *const u8,
                                            width: u64);
    fn parng_predict_scanline_paeth_32bpp(dest: *mut u8,
                                          src: *const u8,
                                          prev: *const u8,
                                          width: u64);
    fn parng_predict_scanline_paeth_24bpp(dest: *mut u8,
                                          src: *const u8,
                                          prev: *const u8,
                                          width: u64);
}

