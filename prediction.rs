// parng/prediction.rs
//
// Copyright (c) 2016 Mozilla Foundation

use DataProvider;
use LevelOfDetail;
use PngError;
use ScanlineData;
use std::iter;
use std::mem;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

pub enum MainThreadToPredictorThreadMsg {
    /// Sets a new `DataProvider`.
    SetDataProvider(Box<DataProvider>),
    /// Sets a new palette.
    SetPalette(Vec<u8>),
    /// Tells the data provider to extract data.
    ExtractData,
    Predict(PredictionRequest),
}

pub struct PredictionRequest {
    pub width: u32,
    pub height: u32,
    pub color_depth: u8,
    pub indexed_color: bool,
    pub predictor: Predictor,
    pub scanline_data: Vec<u8>,
    pub scanline_offset: usize,
    pub scanline_lod: LevelOfDetail,
    pub scanline_y: u32,
}

pub enum PredictorThreadToMainThreadMsg {
    ScanlineComplete(u32, LevelOfDetail, Vec<u8>),
    NoDataProviderError,
}

pub struct MainThreadToPredictorThreadComm {
    pub sender: Sender<MainThreadToPredictorThreadMsg>,
    pub receiver: Receiver<PredictorThreadToMainThreadMsg>,
    pub scanlines_in_progress: u32,
}

impl MainThreadToPredictorThreadComm {
    pub fn new() -> MainThreadToPredictorThreadComm {
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

fn predictor_thread(sender: Sender<PredictorThreadToMainThreadMsg>,
                    receiver: Receiver<MainThreadToPredictorThreadMsg>) {
    let mut data_provider: Option<Box<DataProvider>> = None;
    let mut palette: Option<Vec<u8>> = None;
    let mut blank = vec![];
    while let Ok(msg) = receiver.recv() {
        match msg {
            MainThreadToPredictorThreadMsg::Predict(PredictionRequest {
                    width,
                    height,
                    color_depth,
                    indexed_color,
                    predictor,
                    scanline_data: src,
                    scanline_offset,
                    scanline_lod,
                    scanline_y
            }) => {
                let data_provider = match data_provider {
                    None => {
                        sender.send(PredictorThreadToMainThreadMsg::NoDataProviderError).unwrap();
                        continue
                    }
                    Some(ref mut data_provider) => data_provider,
                };

                let dest_width_in_bytes = width as usize * 4;

                let prev_scanline_y = if scanline_y == 0 {
                    None
                } else {
                    Some(scanline_y - 1)
                };
                let ScanlineData {
                    reference_scanline: mut prev,
                    current_scanline: dest,
                    stride,
                } = data_provider.get_scanline_data(prev_scanline_y, scanline_y, scanline_lod);
                let mut properly_aligned = true;
                let prev = match prev {
                    Some(ref mut prev) => {
                        if !slice_is_properly_aligned(prev) {
                            properly_aligned = false;
                        }
                        &mut prev[..]
                    }
                    None => {
                        blank.extend(iter::repeat(0).take(dest_width_in_bytes as usize));
                        &mut blank[..]
                    }
                };
                if !slice_is_properly_aligned(dest) {
                    properly_aligned = false;
                }

                if properly_aligned {
                    predictor.accelerated_predict(&mut dest[..],
                                                  &src[scanline_offset..],
                                                  &prev[..],
                                                  width,
                                                  color_depth,
                                                  stride)
                } else {
                    predictor.predict(&mut dest[0..dest_width_in_bytes],
                                      &src[scanline_offset..],
                                      &prev[0..dest_width_in_bytes],
                                      width,
                                      color_depth,
                                      stride);
                }

                if indexed_color {
                    let palette = palette.as_ref().expect("Indexed color but no palette?!");
                    convert_indexed_to_rgba(&mut prev[0..dest_width_in_bytes], &palette[..]);
                    if scanline_y == height - 1 {
                        convert_indexed_to_rgba(&mut dest[0..dest_width_in_bytes], &palette[..])
                    }
                }

                sender.send(PredictorThreadToMainThreadMsg::ScanlineComplete(scanline_y,
                                                                             scanline_lod,
                                                                             src)).unwrap()
            }
            MainThreadToPredictorThreadMsg::SetDataProvider(new_data_provider) => {
                data_provider = Some(new_data_provider)
            }
            MainThreadToPredictorThreadMsg::SetPalette(new_rgb_palette) => {
                let mut new_rgba_palette = Vec::with_capacity(256 * 4);
                for color in new_rgb_palette.chunks(3) {
                    new_rgba_palette.extend_from_slice(&color[..]);
                    new_rgba_palette.push(0xff);
                }
                palette = Some(new_rgba_palette)
            }
            MainThreadToPredictorThreadMsg::ExtractData => {
                if let Some(ref mut data_provider) = mem::replace(&mut data_provider, None) {
                    data_provider.extract_data()
                }
            }
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(u8)]
pub enum Predictor {
    None = 0,
    Left = 1,
    Up = 2,
    Average = 3,
    Paeth = 4,
}

impl Predictor {
    pub fn from_byte(byte: u8) -> Result<Predictor,PngError> {
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
               dest: &mut [u8],
               src: &[u8],
               prev: &[u8],
               _: u32,
               color_depth: u8,
               stride: u8) {
        let color_depth = (color_depth / 8) as usize;
        let mut a: [u8; 4] = [0; 4];
        let mut c: [u8; 4] = [0; 4];
        let stride = stride as usize;
        match self {
            Predictor::None => {
                for (dest, src) in dest.chunks_mut(stride).zip(src.chunks(color_depth)) {
                    for (dest, src) in dest.iter_mut().take(4).zip(src.iter()) {
                        *dest = *src
                    }
                }
            }
            Predictor::Left => {
                for (dest, src) in dest.chunks_mut(stride).zip(src.chunks(color_depth)) {
                    for (dest, (src, a)) in dest.iter_mut()
                                                .take(4)
                                                .zip(src.iter().zip(a.iter_mut())) {
                        *a = src.wrapping_add(*a);
                        *dest = *a
                    }
                }
            }
            Predictor::Up => {
                for (dest, (src, b)) in dest.chunks_mut(stride).zip(src.chunks(color_depth)
                                                                       .zip(prev.chunks(stride))) {
                    for (dest, (src, b)) in dest.iter_mut()
                                                .take(4)
                                                .zip(src.iter().zip(b.iter().take(4))) {
                        *dest = src.wrapping_add(*b)
                    }
                }
            }
            Predictor::Average => {
                for (dest, (src, b)) in dest.chunks_mut(stride).zip(src.chunks(color_depth)
                                                                       .zip(prev.chunks(stride))) {
                    for (dest, (src, (b, a))) in
                            dest.iter_mut()
                                .take(4)
                                .zip(src.iter().zip(b.iter().take(4).zip(a.iter_mut()))) {
                        *a = src.wrapping_add((((*a as u16) + (*b as u16)) / 2) as u8);
                        *dest = *a
                    }
                }
            }
            Predictor::Paeth => {
                for (dest, (src, b)) in dest.chunks_mut(stride).zip(src.chunks(color_depth)
                                                                       .zip(prev.chunks(stride))) {
                    for (a, (b, (c, (dest, src)))) in
                        a.iter_mut().zip(b.iter().take(4).zip(c.iter_mut()
                                                               .zip(dest.iter_mut()
                                                                        .take(4)
                                                                        .zip(src.iter())))) {
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
                           color_depth: u8,
                           stride: u8) {
        debug_assert!(((dest.as_ptr() as usize) & 0xf) == 0);
        debug_assert!(((src.as_ptr() as usize) & 0xf) == 0);
        debug_assert!(((prev.as_ptr() as usize) & 0xf) == 0);
        debug_assert!(color_depth == 32 || color_depth == 24 || color_depth == 8);

        let accelerated_implementation = match (self, color_depth, stride) {
            (Predictor::None, 32, 4) => Some(parng_predict_scanline_none_packed_32bpp),
            (Predictor::None, 32, _) => Some(parng_predict_scanline_none_strided_32bpp),
            (Predictor::None, 24, 4) => Some(parng_predict_scanline_none_packed_24bpp),
            (Predictor::None, 24, _) => Some(parng_predict_scanline_none_strided_24bpp),
            (Predictor::None, 8, 4) => Some(parng_predict_scanline_none_packed_8bpp),
            (Predictor::None, 8, _) => None,
            (Predictor::Left, 32, 4) => Some(parng_predict_scanline_left_packed_32bpp),
            (Predictor::Left, 32, _) => Some(parng_predict_scanline_left_strided_32bpp),
            (Predictor::Left, 24, 4) => Some(parng_predict_scanline_left_packed_24bpp),
            (Predictor::Left, 24, _) => Some(parng_predict_scanline_left_strided_24bpp),
            (Predictor::Left, 8, 4) => Some(parng_predict_scanline_left_packed_8bpp),
            (Predictor::Left, 8, _) => None,
            (Predictor::Up, 32, 4) => Some(parng_predict_scanline_up_packed_32bpp),
            (Predictor::Up, 32, _) => Some(parng_predict_scanline_up_strided_32bpp),
            (Predictor::Up, 24, 4) => Some(parng_predict_scanline_up_packed_24bpp),
            (Predictor::Up, 24, _) => Some(parng_predict_scanline_up_strided_24bpp),
            (Predictor::Up, 8, 4) => Some(parng_predict_scanline_up_packed_8bpp),
            (Predictor::Up, 8, _) => None,
            (Predictor::Average, 32, _) => Some(parng_predict_scanline_average_strided_32bpp),
            (Predictor::Average, 24, _) => Some(parng_predict_scanline_average_strided_24bpp),
            (Predictor::Average, 8, _) => None,
            (Predictor::Paeth, 32, _) => Some(parng_predict_scanline_paeth_strided_32bpp),
            (Predictor::Paeth, 24, _) => Some(parng_predict_scanline_paeth_strided_24bpp),
            (Predictor::Paeth, 8, _) => None,
            _ => panic!("Unsupported predictor/color depth combination!"),
        };
        match accelerated_implementation {
            Some(accelerated_implementation) => {
                unsafe {
                    accelerated_implementation(dest.as_mut_ptr(),
                                               src.as_ptr(),
                                               prev.as_ptr(),
                                               (width as u64) * 4,
                                               stride as u64)
                }
            }
            None => self.predict(dest, src, prev, width, color_depth, stride),
        }
    }
}

/// TODO(pcwalton): Agner says latency is going down for `vpgatherdd`. I don't have a Skylake to
/// test on, but maybe it's worth using that instruction on that model and later?
fn convert_indexed_to_rgba(scanline: &mut [u8], palette: &[u8]) {
    for color in scanline.chunks_mut(4) {
        let start = 4 * (color[0] as usize);
        color.clone_from_slice(&palette[start..(start + 4)])
    }
}

fn slice_is_properly_aligned(buffer: &[u8]) -> bool {
    address_is_properly_aligned(buffer.as_ptr() as usize) &&
        address_is_properly_aligned(buffer.len())
}

fn address_is_properly_aligned(address: usize) -> bool {
    (address & 0xf) == 0
}

#[link(name="parngacceleration")]
extern {
    fn parng_predict_scanline_none_packed_32bpp(dest: *mut u8,
                                                src: *const u8,
                                                prev: *const u8,
                                                length: u64,
                                                stride: u64);
    fn parng_predict_scanline_none_strided_32bpp(dest: *mut u8,
                                                 src: *const u8,
                                                 prev: *const u8,
                                                 length: u64,
                                                 stride: u64);
    fn parng_predict_scanline_none_packed_24bpp(dest: *mut u8,
                                                src: *const u8,
                                                prev: *const u8,
                                                length: u64,
                                                stride: u64);
    fn parng_predict_scanline_none_strided_24bpp(dest: *mut u8,
                                                 src: *const u8,
                                                 prev: *const u8,
                                                 length: u64,
                                                 stride: u64);
    fn parng_predict_scanline_none_packed_8bpp(dest: *mut u8,
                                               src: *const u8,
                                               prev: *const u8,
                                               length: u64,
                                               stride: u64);
    fn parng_predict_scanline_left_packed_32bpp(dest: *mut u8,
                                                src: *const u8,
                                                prev: *const u8,
                                                length: u64,
                                                stride: u64);
    fn parng_predict_scanline_left_strided_32bpp(dest: *mut u8,
                                                 src: *const u8,
                                                 prev: *const u8,
                                                 length: u64,
                                                 stride: u64);
    fn parng_predict_scanline_left_packed_24bpp(dest: *mut u8,
                                                src: *const u8,
                                                prev: *const u8,
                                                length: u64,
                                                stride: u64);
    fn parng_predict_scanline_left_strided_24bpp(dest: *mut u8,
                                                 src: *const u8,
                                                 prev: *const u8,
                                                 length: u64,
                                                 stride: u64);
    fn parng_predict_scanline_left_packed_8bpp(dest: *mut u8,
                                               src: *const u8,
                                               prev: *const u8,
                                               length: u64,
                                               stride: u64);
    fn parng_predict_scanline_up_packed_32bpp(dest: *mut u8,
                                              src: *const u8,
                                              prev: *const u8,
                                              length: u64,
                                              stride: u64);
    fn parng_predict_scanline_up_strided_32bpp(dest: *mut u8,
                                               src: *const u8,
                                               prev: *const u8,
                                               length: u64,
                                               stride: u64);
    fn parng_predict_scanline_up_packed_24bpp(dest: *mut u8,
                                              src: *const u8,
                                              prev: *const u8,
                                              length: u64,
                                              stride: u64);
    fn parng_predict_scanline_up_strided_24bpp(dest: *mut u8,
                                               src: *const u8,
                                               prev: *const u8,
                                               length: u64,
                                               stride: u64);
    fn parng_predict_scanline_up_packed_8bpp(dest: *mut u8,
                                             src: *const u8,
                                             prev: *const u8,
                                             length: u64,
                                             stride: u64);
    fn parng_predict_scanline_average_strided_32bpp(dest: *mut u8,
                                                    src: *const u8,
                                                    prev: *const u8,
                                                    length: u64,
                                                    stride: u64);
    fn parng_predict_scanline_average_strided_24bpp(dest: *mut u8,
                                                    src: *const u8,
                                                    prev: *const u8,
                                                    length: u64,
                                                    stride: u64);
    fn parng_predict_scanline_paeth_strided_32bpp(dest: *mut u8,
                                                  src: *const u8,
                                                  prev: *const u8,
                                                  length: u64,
                                                  stride: u64);
    fn parng_predict_scanline_paeth_strided_24bpp(dest: *mut u8,
                                                  src: *const u8,
                                                  prev: *const u8,
                                                  length: u64,
                                                  stride: u64);
}

