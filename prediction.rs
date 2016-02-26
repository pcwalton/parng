// parng/prediction.rs
//
// Copyright (c) 2016 Mozilla Foundation

use PngError;
use imageloader::{DataProvider, LevelOfDetail, ScanlinesForPrediction, ScanlinesForRgbaConversion};
use imageloader::{Transparency};
use std::iter;
use std::mem;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

static NO_LEVELS_OF_DETAIL: [LevelOfDetail; 1] = [LevelOfDetail::None];
static ADAM7_LEVELS_OF_DETAIL: [LevelOfDetail; 7] = [
    LevelOfDetail::Adam7(0),
    LevelOfDetail::Adam7(1),
    LevelOfDetail::Adam7(2),
    LevelOfDetail::Adam7(3),
    LevelOfDetail::Adam7(4),
    LevelOfDetail::Adam7(5),
    LevelOfDetail::Adam7(6),
];

pub enum MainThreadToPredictorThreadMsg {
    /// Sets a new data provider.
    SetDataProvider(Box<DataProvider>),
    /// The image is finished entropy decoding.
    Finished,
    Predict(PredictionRequest),
    PerformRgbaConversion(PerformRgbaConversionRequest),
}

pub struct PredictionRequest {
    pub width: u32,
    pub height: u32,
    pub color_depth: u8,
    pub indexed_color: bool,
    pub scanlines: Vec<ScanlineToPredict>,
}

pub struct PerformRgbaConversionRequest {
    pub rgb_palette: Vec<u8>,
    pub transparency: Transparency,
    pub width: u32,
    pub height: u32,
    pub color_depth: u8,
    pub interlaced: bool,
}

pub struct ScanlineToPredict {
    pub predictor: Predictor,
    pub data: Vec<u8>,
    pub offset: usize,
    pub lod: LevelOfDetail,
    pub y: u32,
}

pub enum PredictorThreadToMainThreadMsg {
    ScanlinePredictionComplete(u32, LevelOfDetail, Vec<u8>),
    RgbaConversionComplete,
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
                    scanlines,
            }) => {
                let data_provider = match data_provider {
                    None => {
                        sender.send(PredictorThreadToMainThreadMsg::NoDataProviderError).unwrap();
                        continue
                    }
                    Some(ref mut data_provider) => data_provider,
                };

                if !indexed_color {
                    palette = None
                }

                let dest_width_in_bytes = width as usize * 4;

                for ScanlineToPredict {
                    mut predictor,
                    data: src,
                    offset: scanline_offset,
                    lod: scanline_lod,
                    y: scanline_y
                } in scanlines {
                    let prev_scanline_y = if scanline_y == 0 {
                        None
                    } else {
                        Some(scanline_y - 1)
                    };

                    {
                        let ScanlinesForPrediction {
                            reference_scanline: mut prev,
                            current_scanline: dest,
                            stride,
                        } = data_provider.fetch_scanlines_for_prediction(prev_scanline_y,
                                                                         scanline_y,
                                                                         scanline_lod,
                                                                         indexed_color);
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
                                              color_depth,
                                              stride);
                        }

                        if !indexed_color {
                            convert_grayscale_to_rgba(&mut prev[0..dest_width_in_bytes],
                                                      &palette,
                                                      color_depth);
                            if scanline_y == height - 1 {
                                convert_grayscale_to_rgba(&mut dest[0..dest_width_in_bytes],
                                                          &palette,
                                                          color_depth);
                            }
                        }

                        sender.send(PredictorThreadToMainThreadMsg::ScanlinePredictionComplete(
                                scanline_y,
                                scanline_lod,
                                src)).unwrap();
                    }

                    data_provider.prediction_complete_for_scanline(scanline_y, scanline_lod);
                }
            }
            MainThreadToPredictorThreadMsg::SetDataProvider(new_data_provider) => {
                data_provider = Some(new_data_provider)
            }
            MainThreadToPredictorThreadMsg::PerformRgbaConversion(PerformRgbaConversionRequest {
                    rgb_palette,
                    transparency,
                    width,
                    height,
                    color_depth,
                    interlaced
            }) => {
                let data_provider = match data_provider {
                    None => {
                        sender.send(PredictorThreadToMainThreadMsg::NoDataProviderError).unwrap();
                        continue
                    }
                    Some(ref mut data_provider) => data_provider,
                };
                let levels_of_detail = if !interlaced {
                    &NO_LEVELS_OF_DETAIL[..]
                } else {
                    &ADAM7_LEVELS_OF_DETAIL[..]
                };

                for lod in levels_of_detail {
                    for scanline_y in 0..height {
                        {
                            let ScanlinesForRgbaConversion {
                                rgba_scanline: dest,
                                indexed_scanline: src,
                                rgba_stride: dest_stride,
                                indexed_stride: src_stride,
                            } = data_provider.fetch_scanlines_for_rgba_conversion(scanline_y,
                                                                                  *lod);
                            let dest_line_stride = (dest_stride as usize) * (width as usize);
                            let src_line_stride = (src_stride as usize) * (width as usize);
                            convert_indexed_to_rgba(&mut dest[0..dest_line_stride],
                                                    &src[0..src_line_stride],
                                                    &rgb_palette[..],
                                                    &transparency,
                                                    color_depth,
                                                    dest_stride,
                                                    src_stride);
                        }

                        data_provider.rgba_conversion_complete_for_scanline(scanline_y, *lod);
                    }
                }

                sender.send(PredictorThreadToMainThreadMsg::RgbaConversionComplete).unwrap();
            }
            MainThreadToPredictorThreadMsg::Finished => {
                if let Some(ref mut data_provider) = mem::replace(&mut data_provider, None) {
                    data_provider.finished()
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

    fn predict(self, dest: &mut [u8], src: &[u8], prev: &[u8], color_depth: u8, stride: u8) {
        let color_depth = (color_depth / 8) as usize;
        let mut a: [u8; 4] = [0; 4];
        let mut c: [u8; 4] = [0; 4];
        let stride = stride as usize;

        // We use iterators here to avoid bounds checks, as this is performance-critical code.
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
        debug_assert!([8, 16, 24, 32].contains(&color_depth));

        let accelerated_implementation = match (self, color_depth, stride) {
            (Predictor::None, 32, 4) => Some(parng_predict_scanline_none_packed_32bpp),
            (Predictor::None, 32, _) => Some(parng_predict_scanline_none_strided_32bpp),
            (Predictor::None, 24, 4) => Some(parng_predict_scanline_none_packed_24bpp),
            (Predictor::None, 24, _) => Some(parng_predict_scanline_none_strided_24bpp),
            (Predictor::None, 16, 4) => Some(parng_predict_scanline_none_packed_16bpp),
            (Predictor::None, 16, _) => None,
            (Predictor::None, 8, 4) => Some(parng_predict_scanline_none_packed_8bpp),
            (Predictor::None, 8, _) => None,
            (Predictor::Left, 32, 4) => Some(parng_predict_scanline_left_packed_32bpp),
            (Predictor::Left, 32, _) => Some(parng_predict_scanline_left_strided_32bpp),
            (Predictor::Left, 24, 4) => Some(parng_predict_scanline_left_packed_24bpp),
            (Predictor::Left, 24, _) => Some(parng_predict_scanline_left_strided_24bpp),
            (Predictor::Left, 16, 4) => Some(parng_predict_scanline_left_packed_16bpp),
            (Predictor::Left, 16, _) => None,
            (Predictor::Left, 8, 4) => Some(parng_predict_scanline_left_packed_8bpp),
            (Predictor::Left, 8, _) => None,
            (Predictor::Up, 32, 4) => Some(parng_predict_scanline_up_packed_32bpp),
            (Predictor::Up, 32, _) => Some(parng_predict_scanline_up_strided_32bpp),
            (Predictor::Up, 24, 4) => Some(parng_predict_scanline_up_packed_24bpp),
            (Predictor::Up, 24, _) => Some(parng_predict_scanline_up_strided_24bpp),
            (Predictor::Up, 16, 4) => Some(parng_predict_scanline_up_packed_16bpp),
            (Predictor::Up, 16, _) => None,
            (Predictor::Up, 8, 4) => Some(parng_predict_scanline_up_packed_8bpp),
            (Predictor::Up, 8, _) => None,
            (Predictor::Average, 32, _) => Some(parng_predict_scanline_average_strided_32bpp),
            (Predictor::Average, 24, _) => Some(parng_predict_scanline_average_strided_24bpp),
            (Predictor::Average, 16, _) => None,
            (Predictor::Average, 8, _) => None,
            (Predictor::Paeth, 32, _) => Some(parng_predict_scanline_paeth_strided_32bpp),
            (Predictor::Paeth, 24, _) => Some(parng_predict_scanline_paeth_strided_24bpp),
            (Predictor::Paeth, 16, _) => None,
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
            None => self.predict(dest, src, prev, color_depth, stride),
        }
    }
}

fn convert_grayscale_to_rgba(scanline: &mut [u8], palette: &Option<Vec<u8>>, color_depth: u8) {
    // TODO(pcwalton): Support 1bpp, 2bpp, and 4bpp grayscale.
    match color_depth {
        32 | 24 => {}
        16 => convert_grayscale_alpha_to_rgba(scanline),
        8 => convert_8bpp_grayscale_to_rgba(scanline),
        _ => panic!("convert_to_rgba: Unsupported color depth!"),
    }
}

/// TODO(pcwalton): Agner says latency is going down for `vpgatherdd`. I don't have a Skylake to
/// test on, but maybe it's worth using that instruction on that model and later?
fn convert_indexed_to_rgba(dest: &mut [u8],
                           src: &[u8],
                           rgb_palette: &[u8],
                           transparency: &Transparency,
                           _: u8,
                           dest_stride: u8,
                           src_stride: u8) {
    // TODO(pcwalton): Support 1bpp, 2bpp, and 4bpp indexed color.
    for (dest, src) in dest.chunks_mut(dest_stride as usize).zip(src.chunks(src_stride as usize)) {
        let start = 3 * (src[0] as usize);
        dest[0..3].clone_from_slice(&rgb_palette[start..(start + 3)]);
        dest[3] = match *transparency {
            Transparency::None => 0xff,
            Transparency::Indexed(ref palette) => {
                let index = src[0] as usize;
                if index < palette.len() {
                    palette[index]
                } else {
                    0xff
                }
            }
            Transparency::MagicColor(..) => {
                panic!("Can't have magic color transparency in indexed color images!")
            }
        }
    }
}

/// TODO(pcwalton): Use SIMD for this. Greyscale images are pretty rare, so it's not a priority,
/// but it would be nice.
fn convert_grayscale_alpha_to_rgba(scanline: &mut [u8]) {
    for color in scanline.chunks_mut(4) {
        let (y, a) = (color[0], color[1]);
        color[1] = y;
        color[2] = y;
        color[3] = a
    }
}

/// TODO(pcwalton): Use SIMD for this too.
fn convert_8bpp_grayscale_to_rgba(scanline: &mut [u8]) {
    for color in scanline.chunks_mut(4) {
        let y = color[0];
        color[1] = y;
        color[2] = y;
        color[3] = y
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
    fn parng_predict_scanline_none_packed_16bpp(dest: *mut u8,
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
    fn parng_predict_scanline_left_packed_16bpp(dest: *mut u8,
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
    fn parng_predict_scanline_up_packed_16bpp(dest: *mut u8,
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

