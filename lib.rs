//! A parallel PNG decoder.

extern crate byteorder;
extern crate libc;
extern crate libz_sys;
extern crate num;

use libc::c_int;
use libz_sys::{Z_ERRNO, Z_NO_FLUSH, Z_OK, Z_STREAM_END, z_stream};
use metadata::{ChunkHeader, Metadata};
use std::io::{self, BufRead, Seek, SeekFrom};
use std::mem;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

pub mod metadata;

pub struct Image {
    metadata: Option<Metadata>,
    z_stream: z_stream,
    compressed_data_buffer: Vec<u8>,
    predictor_buffer: u8,
    scanline_data_buffer: Vec<u8>,
    scanline_data_buffer_full: bool,
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
            predictor_buffer: 0,
            scanline_data_buffer: vec![],
            scanline_data_buffer_full: false,
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
                    if self.scanline_data_buffer_full {
                        println!("buffer full!");
                        return Ok(AddDataResult::BufferFull)
                    }

                    let original_compressed_data_buffer_length =
                        self.compressed_data_buffer.len() as u32;
                    self.compressed_data_buffer.resize(
                        (original_compressed_data_buffer_length + bytes_left_in_chunk) as usize,
                        0);

                    let byte_count = {
                        let mut compressed_data_buffer = &mut self.compressed_data_buffer[
                            (original_compressed_data_buffer_length as usize)..];
                        original_compressed_data_buffer_length +
                            try!(reader.read(compressed_data_buffer).map_err(PngError::Io)) as u32
                    };

                    unsafe {
                        self.z_stream.avail_in = byte_count as u32;
                        // FIXME(pcwalton): The below line is totally bogus! We need to keep
                        // track of how far we are in the buffer.
                        self.z_stream.next_in = &mut self.compressed_data_buffer[0];

                        // Read the predictor byte.
                        // TODO(pcwalton): Improve this. This is probably going to show up in
                        // profiles. SSE alignment restrictions make this annoying, though.
                        if self.z_stream.avail_in != 0 {
                            self.z_stream.avail_out = 1;
                            self.z_stream.next_out = &mut self.predictor_buffer;
                            try!(PngError::from_zlib_result(libz_sys::inflate(
                                        &mut self.z_stream,
                                        Z_NO_FLUSH)));
                            if self.z_stream.avail_out != 0 {
                                println!("returning out, couldn't read predictor byte");
                                return Ok((AddDataResult::Continue))
                            }

                            println!("read predictor byte!");

                            // Read the scanline data.
                            //
                            // TODO(pcwalton): This may well show up in profiles too. Probably we
                            // are going to want to read multiple scanlines at once. Again, before
                            // we do this, though, we are going to have to deal with SSE alignment
                            // restrictions.
                            let stride = self.stride();
                            if self.z_stream.avail_in != 0 {
                                self.scanline_data_buffer.truncate(0);
                                self.scanline_data_buffer.reserve(stride as usize);
                                self.z_stream.avail_out = stride;
                                self.z_stream.next_out = self.scanline_data_buffer.as_mut_ptr();
                                try!(PngError::from_zlib_result(libz_sys::inflate(
                                            &mut self.z_stream,
                                            Z_NO_FLUSH)));
                                if self.z_stream.avail_out != 0 {
                                    println!("read scanline but avail_out is {}",
                                             self.z_stream.avail_out);
                                    return Ok((AddDataResult::Continue))
                                } else {
                                    self.scanline_data_buffer.set_len(stride as usize);
                                    self.scanline_data_buffer_full = true
                                }
                            } else {
                                println!("no avail_in B, byte_count {}", byte_count);
                                return Ok((AddDataResult::Continue))
                            }
                        } else {
                            println!("no avail_in A, byte_count {}", byte_count);
                            return Ok((AddDataResult::Continue))
                        }
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
                DecodeState::Finished => {
                    println!("returning finished!");
                    return Ok(AddDataResult::Finished)
                }
            }
        }
    }

    #[inline(never)]
    pub fn decode(&mut self) -> Result<DecodeResult,PngError> {
        if self.predictor_thread_comm.is_none() {
            self.predictor_thread_comm = Some(MainThreadToPredictorThreadComm::new())
        }
        let predictor_thread_comm = self.predictor_thread_comm.as_mut().unwrap();

        let result_buffer = if predictor_thread_comm.is_busy {
            println!("waiting for result buffer...");
            predictor_thread_comm.is_busy = false;
            let result = Some(predictor_thread_comm.receiver.recv().unwrap().0);
            println!("got result buffer!");
            result
        } else {
            println!("no result buffer!");
            None
        };

        if self.scanline_data_buffer_full {
            let msg = MainThreadToPredictorThreadMsg::Predict(
                try!(Predictor::from_byte(self.predictor_buffer)),
                mem::replace(&mut self.scanline_data_buffer, vec![]));
            predictor_thread_comm.sender.send(msg).unwrap();
            predictor_thread_comm.is_busy = true;
            self.scanline_data_buffer_full = false
        }

        match result_buffer {
            Some(result_buffer) => {
                self.scanline_data_buffer = result_buffer;
                Ok(DecodeResult::Scanline(&mut self.scanline_data_buffer[..]))
            }
            None => Ok(DecodeResult::None),
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
}

enum MainThreadToPredictorThreadMsg {
    Predict(Predictor, Vec<u8>),
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
            // TODO(pcwalton): Support other color depths!
            predictor_thread(32,
                             predictor_thread_to_main_thread_sender,
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
    Scanline(&'a mut [u8]),
}

fn predictor_thread(color_depth: u8,
                    sender: Sender<PredictorThreadToMainThreadMsg>,
                    receiver: Receiver<MainThreadToPredictorThreadMsg>) {
    let mut prev = vec![];
    while let Ok(msg) = receiver.recv() {
        match msg {
            MainThreadToPredictorThreadMsg::Predict(predictor, mut scanline) => {
                let stride = scanline.len();
                let width = stride / (color_depth as usize / 8);
                if prev.len() != stride {
                    prev = scanline.iter().map(|_| 0).collect();
                }
                let decode_scanline = match predictor {
                    Predictor::None => parng_predict_scanline_none,
                    Predictor::Left => parng_predict_scanline_left,
                    Predictor::Up => parng_predict_scanline_up,
                    Predictor::Average => parng_predict_scanline_average,
                    Predictor::Paeth => parng_predict_scanline_paeth,
                };
                unsafe {
                    decode_scanline(&mut scanline[0], &prev[0], width as u64)
                }
                // FIXME(pcwalton): Any way to avoid this copy?
                prev[..].clone_from_slice(&mut scanline[..]);
                sender.send(PredictorThreadToMainThreadMsg(scanline)).unwrap()
            }
        }
    }
}

#[allow(non_snake_case)]
unsafe fn inflateInit(strm: *mut z_stream) -> c_int {
    let version = libz_sys::zlibVersion();
    libz_sys::inflateInit_(strm, version, mem::size_of::<z_stream>() as c_int)
}

#[link(name="parngpredict")]
extern {
    fn parng_predict_scanline_none(this: *mut u8, prev: *const u8, width: u64);
    fn parng_predict_scanline_left(this: *mut u8, prev: *const u8, width: u64);
    fn parng_predict_scanline_up(this: *mut u8, prev: *const u8, width: u64);
    fn parng_predict_scanline_average(this: *mut u8, prev: *const u8, width: u64);
    fn parng_predict_scanline_paeth(this: *mut u8, prev: *const u8, width: u64);
}

