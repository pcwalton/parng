//! A parallel PNG decoder.

extern crate byteorder;
extern crate libc;
extern crate libz_sys;
extern crate num;

use libc::c_int;
use libz_sys::{Z_ERRNO, Z_NO_FLUSH, Z_OK, Z_STREAM_END, z_stream};
use metadata::{ChunkHeader, InterlaceMethod, Metadata};
use std::cmp;
use std::io::{self, BufRead, Seek, SeekFrom};
use std::mem;
use std::ptr;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

const BUFFER_SIZE: usize = 16384;

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
                       where R: BufRead + Seek {
        loop {
            let initial_pos = try!(reader.seek(SeekFrom::Current(0)).map_err(PngError::Io));
            match self.decode_state {
                DecodeState::Start => {
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
                            let original_size = self.scanline_data_buffer_size;
                            self.scanline_data_buffer.resize(1 + stride as usize, 0);
                            self.z_stream.avail_out = 1 + stride - (original_size as u32);
                            self.z_stream.next_out = &mut self.scanline_data_buffer[original_size];
                            debug_assert!(self.z_stream.avail_out as usize + original_size <=
                                          self.scanline_data_buffer.len());
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
                    println!("Finished!");
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
        let width_and_stride_for_scanline_data_buffer_if_full =
            self.scanline_data_buffer_lod.map(|lod| {
                (self.width_for_lod(lod), self.stride_for_lod(lod))
            });

        if self.predictor_thread_comm.is_none() {
            self.predictor_thread_comm = Some(MainThreadToPredictorThreadComm::new())
        }
        let predictor_thread_comm = self.predictor_thread_comm.as_mut().unwrap();

        let result_buffer = if predictor_thread_comm.is_busy {
            Some(predictor_thread_comm.receiver.recv().unwrap().0)
        } else {
            None
        };
        predictor_thread_comm.is_busy = false;

        if let Some((scanline_width, scanline_stride)) =
                width_and_stride_for_scanline_data_buffer_if_full {
            // FIXME(pcwalton): This is killing our performance. Stop doing it.
            let predictor = self.scanline_data_buffer[0];
            for i in 0..(scanline_stride as usize) {
                self.scanline_data_buffer[i] = self.scanline_data_buffer[i + 1]
            }
            self.scanline_data_buffer.pop();

            let msg = MainThreadToPredictorThreadMsg::Predict(
                scanline_width,
                self.metadata.as_ref().unwrap().color_depth,
                try!(Predictor::from_byte(predictor)),
                mem::replace(&mut self.scanline_data_buffer, vec![]));
            self.scanline_data_buffer_size = 0;
            predictor_thread_comm.sender.send(msg).unwrap();
            predictor_thread_comm.is_busy = true
        }
        self.scanline_data_buffer_lod = None;

        match result_buffer {
            Some(result_buffer) => {
                // FIXME(pcwalton): The LOD returned here is wrong!
                self.scanline_data_buffer = result_buffer;
                Ok(DecodeResult::Scanline(&mut self.scanline_data_buffer[..], self.current_lod))
            }
            None => Ok(DecodeResult::None),
        }
    }

    #[inline]
    pub fn metadata(&self) -> &Option<Metadata> {
        &self.metadata
    }

    fn width_for_lod(&self, lod: LevelOfDetail) -> u32 {
        let metadata = self.metadata.as_ref().unwrap();
        let image_width = metadata.dimensions.width;
        match lod {
            LevelOfDetail::Adam7(0) | LevelOfDetail::Adam7(1) => image_width / 8,
            LevelOfDetail::Adam7(2) | LevelOfDetail::Adam7(3) => image_width / 4,
            LevelOfDetail::Adam7(4) | LevelOfDetail::Adam7(5) => image_width / 2,
            _ => image_width,
        }
    }

    fn height_for_lod(&self, lod: LevelOfDetail) -> u32 {
        let metadata = self.metadata.as_ref().unwrap();
        let image_height = metadata.dimensions.height;
        match lod {
            LevelOfDetail::Adam7(0) |
            LevelOfDetail::Adam7(1) |
            LevelOfDetail::Adam7(2) => image_height / 8,
            LevelOfDetail::Adam7(3) | LevelOfDetail::Adam7(4) => image_height / 4,
            LevelOfDetail::Adam7(5) => image_height / 2,
            _ => image_height,
        }
    }

    fn stride_for_lod(&self, lod: LevelOfDetail) -> u32 {
        let metadata = self.metadata.as_ref().unwrap();
        let color_depth = metadata.color_depth;
        self.width_for_lod(lod) * ((color_depth / 8) as u32)
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

    fn predict(self,
               this: &mut [u8],
               prev: &[u8],
               width: u32,
               color_depth: u8,
               mut a: [u8; 4],
               mut c: [u8; 4]) {
        debug_assert!(color_depth == 32);
        match self {
            Predictor::None => {}
            Predictor::Left => {
                for this in this.chunks_mut(4) {
                    for (this, a) in this.iter_mut().zip(a.iter_mut()) {
                        *a = this.wrapping_add(*a);
                        *this = *a
                    }
                }
            }
            Predictor::Up => {
                for (this, b) in this.iter_mut().zip(prev.iter()) {
                    *this = this.wrapping_add(*b)
                }
            }
            Predictor::Average => {
                for (this, b) in this.chunks_mut(4).zip(prev.chunks(4)) {
                    for (this, (b, a)) in this.iter_mut().zip(b.iter().zip(a.iter_mut())) {
                        *a = this.wrapping_add((((*a as u16) + (*b as u16)) / 2) as u8);
                        *this = *a
                    }
                }
            }
            Predictor::Paeth => {
                for (this, b) in this.chunks_mut(4).zip(prev.chunks(4)) {
                    for (a, (b, (c, this))) in a.iter_mut()
                                                .zip(b.iter()
                                                      .zip(c.iter_mut().zip(this.iter_mut()))) {
                        let paeth = paeth(*a, *b, *c);
                        *a = this.wrapping_add(paeth);
                        *c = *b;
                        *this = *a;
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
                           this: &mut [u8],
                           prev: &[u8],
                           width: u32,
                           color_depth: u8,
                           mut a: [u8; 4],
                           mut c: [u8; 4]) {
        // FIXME(pcwalton): For now, we don't support starting with nonzero a or c.
        debug_assert!(a == [0; 4]);
        debug_assert!(c == [0; 4]);
        debug_assert!(((this.as_ptr() as usize) & 0xf) == 0);
        debug_assert!(((prev.as_ptr() as usize) & 0xf) == 0);

        let decode_scanline = match self {
            Predictor::None => return,
            Predictor::Left => parng_predict_scanline_left,
            Predictor::Up => parng_predict_scanline_up,
            Predictor::Average => parng_predict_scanline_average,
            Predictor::Paeth => parng_predict_scanline_paeth,
        };
        unsafe {
            decode_scanline(this.as_mut_ptr(), prev.as_ptr(), width as u64)
        }
    }
}

enum MainThreadToPredictorThreadMsg {
    // Width, color depth, predictor, and scanline data.
    Predict(u32, u8, Predictor, Vec<u8>),
}

struct PredictorThreadToMainThreadMsg(Vec<u8>);

struct MainThreadToPredictorThreadComm {
    sender: Sender<MainThreadToPredictorThreadMsg>,
    receiver: Receiver<PredictorThreadToMainThreadMsg>,
    is_busy: bool,
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
            is_busy: false,
        }
    }
}

#[derive(Debug)]
pub enum DecodeResult<'a> {
    None,
    Scanline(&'a mut [u8], LevelOfDetail),
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum LevelOfDetail {
    None,
    Adam7(u8),
}

fn predictor_thread(sender: Sender<PredictorThreadToMainThreadMsg>,
                    receiver: Receiver<MainThreadToPredictorThreadMsg>) {
    let mut prev = vec![];
    while let Ok(msg) = receiver.recv() {
        match msg {
            MainThreadToPredictorThreadMsg::Predict(width,
                                                    color_depth,
                                                    predictor,
                                                    mut scanline) => {
                let stride = (width as usize) * (color_depth as usize / 8);
                let aligned_stride = if stride % 16 != 0 {
                    stride + 16 - stride % 16
                } else {
                    stride
                };
                while prev.len() < aligned_stride {
                    prev.push(0)
                }
                while scanline.len() < aligned_stride {
                    scanline.push(0)
                }
                match predictor {
                    Predictor::None | Predictor::Left | Predictor::Up | Predictor::Paeth |
                    Predictor::Average => {
                        predictor.accelerated_predict(&mut scanline[..],
                                                      &prev[..],
                                                      width,
                                                      color_depth,
                                                      [0; 4],
                                                      [0; 4])
                    }
                }
                // FIXME(pcwalton): Any way to avoid this copy?
                prev[..].clone_from_slice(&mut scanline[..]);
                sender.send(PredictorThreadToMainThreadMsg(scanline)).unwrap()
            }
        }
    }
}

/// TODO(pcwalton): This could be nicer by allowing images to be rendered with partially-complete
/// LODs.
#[inline(never)]
pub fn deinterlace_adam7(out_scanlines: &mut [u8],
                         in_scanlines: &Adam7Scanlines,
                         width: u32,
                         color_depth: u8) {
    let stride = (width as usize) * (color_depth as usize) / 8;
    assert!(out_scanlines.len() >= 8 * stride);
    assert!(in_scanlines.are_well_formed(stride));
    unsafe {
        parng_deinterlace_adam7_scanline_04(dest(out_scanlines,
                                                 0,
                                                 width,
                                                 color_depth).as_mut_ptr(),
                                            in_scanlines.lod0.as_opt_ptr(0),
                                            in_scanlines.lod1.as_opt_ptr(0),
                                            in_scanlines.lod3.as_opt_ptr(0),
                                            in_scanlines.lod5.as_opt_ptr(0),
                                            width as u64);

        if in_scanlines.lod2.is_some() {
            parng_deinterlace_adam7_scanline_04(dest(out_scanlines,
                                                     4,
                                                     width,
                                                     color_depth).as_mut_ptr(),
                                                in_scanlines.lod2.as_opt_ptr(0),
                                                in_scanlines.lod2.as_opt_ptr(0),
                                                in_scanlines.lod3.as_opt_ptr(1),
                                                in_scanlines.lod5.as_opt_ptr(1),
                                                width as u64);

            if in_scanlines.lod4.is_some() {
                parng_deinterlace_adam7_scanline_26(dest(out_scanlines,
                                                         2,
                                                         width,
                                                         color_depth).as_mut_ptr(),
                                                    in_scanlines.lod4.as_opt_ptr(0),
                                                    in_scanlines.lod5.as_opt_ptr(0),
                                                    width as u64);
                parng_deinterlace_adam7_scanline_26(dest(out_scanlines,
                                                         2,
                                                         width,
                                                         color_depth).as_mut_ptr(),
                                                    in_scanlines.lod4.as_opt_ptr(1),
                                                    in_scanlines.lod5.as_opt_ptr(1),
                                                    width as u64);

                match in_scanlines.lod6 {
                    Some(ref lod6) => {
                        copy_scanline_to_dest(dest(out_scanlines, 1, width, color_depth),
                                              &lod6[0][..],
                                              width,
                                              color_depth);
                        copy_scanline_to_dest(dest(out_scanlines, 3, width, color_depth),
                                              &lod6[1][..],
                                              width,
                                              color_depth);
                        copy_scanline_to_dest(dest(out_scanlines, 5, width, color_depth),
                                              &lod6[2][..],
                                              width,
                                              color_depth);
                        copy_scanline_to_dest(dest(out_scanlines, 7, width, color_depth),
                                              &lod6[3][..],
                                              width,
                                              color_depth);
                    }
                    None => {
                        duplicate_decoded_scanline(out_scanlines, 1, 0, width, color_depth);
                        duplicate_decoded_scanline(out_scanlines, 3, 2, width, color_depth);
                        duplicate_decoded_scanline(out_scanlines, 5, 4, width, color_depth);
                        duplicate_decoded_scanline(out_scanlines, 7, 6, width, color_depth);
                    }
                }
            } else {
                duplicate_decoded_scanline(out_scanlines, 2, 0, width, color_depth);
                duplicate_decoded_scanline(out_scanlines, 6, 4, width, color_depth);
            }
        } else {
            duplicate_decoded_scanline(out_scanlines, 4, 0, width, color_depth)
        }
    }

    fn stride_for_width_and_color_depth(width: u32, color_depth: u8) -> usize {
        (width as usize) * (color_depth as usize) / 8
    }

    fn dest_index(y: u8, width: u32, color_depth: u8) -> usize {
        (y as usize) * stride_for_width_and_color_depth(width, color_depth)
    }

    fn dest(out_scanlines: &mut [u8], y: u8, width: u32, color_depth: u8) -> &mut [u8] {
        let start = dest_index(y, width, color_depth);
        let end = dest_index(y + 1, width, color_depth);
        &mut out_scanlines[start..end]
    }

    fn copy_scanline_to_dest(dest: &mut [u8], src: &[u8], width: u32, color_depth: u8) {
        let stride = stride_for_width_and_color_depth(width, color_depth);
        dest[0..stride].clone_from_slice(&src[0..stride])
    }

    fn duplicate_decoded_scanline(out_scanlines: &mut [u8],
                                  dest_y: u8,
                                  src_y: u8,
                                  width: u32,
                                  color_depth: u8) {
        debug_assert!(dest_y > src_y);
        let (head, tail) = out_scanlines.split_at_mut(dest_index(dest_y, width, color_depth));
        let src_start = dest_index(src_y, width, color_depth);
        copy_scanline_to_dest(tail, &head[src_start..], width, color_depth)
    }
}

pub struct Adam7Scanlines<'a> {
    pub lod0: [&'a [u8]; 1],            // width / 8
    pub lod1: Option<[&'a [u8]; 1]>,    // width / 8
    pub lod2: Option<[&'a [u8]; 1]>,    // width / 4
    pub lod3: Option<[&'a [u8]; 2]>,    // width / 4
    pub lod4: Option<[&'a [u8]; 2]>,    // width / 2
    pub lod5: Option<[&'a [u8]; 4]>,    // width / 2
    pub lod6: Option<[&'a [u8]; 4]>,    // width
}

impl<'a> Adam7Scanlines<'a> {
    // NB: This must be correct for memory safety!
    pub fn are_well_formed(&self, stride: usize) -> bool {
        self.lod0[0].len() >= stride / 8 &&
            self.lod1.are_well_formed(stride / 8) &&
            self.lod2.are_well_formed(stride / 4) &&
            self.lod3.are_well_formed(stride / 4) &&
            self.lod4.are_well_formed(stride / 2) &&
            self.lod5.are_well_formed(stride / 2) &&
            self.lod6.are_well_formed(stride)
    }
}

trait ScanlinesForLod {
    fn as_opt_ptr(&self, index: u8) -> *const u8;
    fn are_well_formed(&self, stride: usize) -> bool;
}

impl<'a> ScanlinesForLod for [&'a [u8]; 1] {
    fn as_opt_ptr(&self, index: u8) -> *const u8 {
        self[index as usize].as_ptr()
    }
    fn are_well_formed(&self, stride: usize) -> bool {
        self.iter().all(|scanline| scanline.len() >= stride)
    }
}

impl<'a> ScanlinesForLod for Option<[&'a [u8]; 1]> {
    fn as_opt_ptr(&self, index: u8) -> *const u8 {
        match *self {
            None => ptr::null(),
            Some(ref buffer) => buffer[index as usize].as_ptr(),
        }
    }
    fn are_well_formed(&self, stride: usize) -> bool {
        match *self {
            None => true,
            Some(ref scanlines) => scanlines.iter().all(|scanline| scanline.len() >= stride),
        }
    }
}

impl<'a> ScanlinesForLod for Option<[&'a [u8]; 2]> {
    fn as_opt_ptr(&self, index: u8) -> *const u8 {
        match *self {
            None => ptr::null(),
            Some(ref buffer) => buffer[index as usize].as_ptr(),
        }
    }
    fn are_well_formed(&self, stride: usize) -> bool {
        match *self {
            None => true,
            Some(ref scanlines) => scanlines.iter().all(|scanline| scanline.len() >= stride),
        }
    }
}

impl<'a> ScanlinesForLod for Option<[&'a [u8]; 4]> {
    fn as_opt_ptr(&self, index: u8) -> *const u8 {
        match *self {
            None => ptr::null(),
            Some(ref buffer) => buffer[index as usize].as_ptr(),
        }
    }
    fn are_well_formed(&self, stride: usize) -> bool {
        match *self {
            None => true,
            Some(ref scanlines) => scanlines.iter().all(|scanline| scanline.len() >= stride),
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
    fn parng_deinterlace_adam7_scanline_04(dest: *mut u8,
                                           lod0: *const u8,
                                           lod1: *const u8,
                                           lod3: *const u8,
                                           lod5: *const u8,
                                           width: u64);
    fn parng_deinterlace_adam7_scanline_26(dest: *mut u8,
                                           lod4: *const u8,
                                           lod5: *const u8,
                                           width: u64);
    fn parng_predict_scanline_left(this: *mut u8, prev: *const u8, width: u64);
    fn parng_predict_scanline_up(this: *mut u8, prev: *const u8, width: u64);
    fn parng_predict_scanline_average(this: *mut u8, prev: *const u8, width: u64);
    fn parng_predict_scanline_paeth(this: *mut u8, prev: *const u8, width: u64);
}

