//! An example showing use of `parng`.

extern crate byteorder;
extern crate clap;
extern crate parng;
extern crate time;

use byteorder::{LittleEndian, WriteBytesExt};
use clap::{App, Arg};
use parng::imageloader::{self, AddDataResult, DataProvider, ImageLoader, InterlacingInfo};
use parng::imageloader::{LevelOfDetail, ScanlineData, UninitializedExtension};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::mem;
use std::sync::mpsc::{self, Receiver, Sender};

const OUTPUT_BPP: u32 = 4;
const RUNS: u32 = 16;

struct SlurpingDataProvider {
    data: Vec<u8>,
    aligned_stride: usize,
    data_sender: Sender<Vec<u8>>,
}

impl SlurpingDataProvider {
    #[inline(never)]
    pub fn new(width: u32, height: u32) -> (SlurpingDataProvider, Receiver<Vec<u8>>) {
        let aligned_stride = imageloader::align(width as usize * 4);
        let (data_sender, data_receiver) = mpsc::channel();
        let length = aligned_stride * (height as usize);
        let mut data = vec![];
        unsafe {
            data.extend_with_uninitialized(length)
        }

        // FIXME(pcwalton): Remove!
        for p in data.iter_mut() {
            *p = 0;
        }

        let data_provider = SlurpingDataProvider {
            data: data,
            aligned_stride: aligned_stride,
            data_sender: data_sender,
        };
        (data_provider, data_receiver)
    }
}

impl DataProvider for SlurpingDataProvider {
    fn get_scanline_data<'a>(&'a mut self,
                             reference_scanline: Option<u32>,
                             current_scanline: u32,
                             lod: LevelOfDetail)
                             -> ScanlineData {
        let reference_scanline = reference_scanline.map(|reference_scanline| {
            InterlacingInfo::new(reference_scanline, 32, lod)
        });
        let current_scanline = InterlacingInfo::new(current_scanline, 32, lod);

        let aligned_stride = self.aligned_stride;
        let split_point = aligned_stride * (current_scanline.y as usize);
        let (head, tail) = self.data.split_at_mut(split_point);
        let head_length = head.len();
        let reference_scanline_data = match reference_scanline {
            None => None,
            Some(reference_scanline) => {
                debug_assert!(current_scanline.stride == reference_scanline.stride);
                let start = (reference_scanline.y as usize) * aligned_stride +
                    (reference_scanline.offset as usize);
                let end = start + aligned_stride;
                let slice = &mut head[start..end];
                Some(slice)
            }
        };
        let start = (current_scanline.y as usize) * aligned_stride +
            (current_scanline.offset as usize) - head_length;
        let end = start + aligned_stride;
        let current_scanline_data = &mut tail[start..end];
        ScanlineData {
            reference_scanline: reference_scanline_data,
            current_scanline: current_scanline_data,
            stride: current_scanline.stride,
        }
    }

    fn extract_data(&mut self) {
        self.data_sender.send(mem::replace(&mut self.data, vec![])).unwrap()
    }
}

#[inline(never)]
fn get_data_from_receiver(data_receiver: Receiver<Vec<u8>>) -> Vec<u8> {
    data_receiver.recv().unwrap()
}

#[inline(never)]
fn decode(image: &mut ImageLoader, input: &mut File, width: u32, height: u32) -> Vec<u8> {
    let (data_provider, data_receiver) = SlurpingDataProvider::new(width, height);
    image.set_data_provider(Box::new(data_provider));

    while let AddDataResult::Continue = image.add_data(input).unwrap() {}
    image.wait_until_finished().unwrap();
    image.extract_data();
    get_data_from_receiver(data_receiver)
}

fn main() {
    let matches = App::new("example").arg(Arg::with_name("INPUT").required(true))
                                     .arg(Arg::with_name("OUTPUT").required(true))
                                     .get_matches();

    let in_path = matches.value_of("INPUT").unwrap();
    let out_path = matches.value_of("OUTPUT").unwrap();

    let mut total_elapsed_time = 0;
    let mut dimensions = None;
    let mut pixels = None;
    for _ in 0..RUNS {
        let before = time::precise_time_ns();
        let mut input = File::open(in_path).unwrap();
        let mut image = ImageLoader::new().unwrap();
        loop {
            match image.add_data(&mut input).unwrap() {
                AddDataResult::Continue => {
                    if image.metadata().is_some() {
                        break
                    }
                }
                AddDataResult::Finished => panic!("Image ended before metadata was read!"),
            }
        }
        dimensions = Some(image.metadata().as_ref().unwrap().dimensions);
        pixels = Some(decode(&mut image,
                             &mut input,
                             dimensions.unwrap().width,
                             dimensions.unwrap().height));
        let elapsed = time::precise_time_ns() - before;
        total_elapsed_time += elapsed;
    }

    total_elapsed_time /= RUNS as u64;
    println!("Mean elapsed time: {}ms", total_elapsed_time as f32 / 1_000_000.0);

    let dimensions = dimensions.unwrap();
    let pixels = pixels.unwrap();

    let mut output = BufWriter::new(File::create(out_path).unwrap());
    output.write_all(&[0, 0, 2, 0,
                       0, 0, 0, 0,
                       0, 0, 0, 0]).unwrap();
    output.write_u16::<LittleEndian>(dimensions.width as u16).unwrap();
    output.write_u16::<LittleEndian>(dimensions.height as u16).unwrap();
    output.write_all(&[24, 0]).unwrap();

    let aligned_stride = imageloader::align(dimensions.width as usize * 4);
    for y in 0..dimensions.height {
        let y = dimensions.height - y - 1;
        for x in 0..dimensions.width {
            let start = (aligned_stride * (y as usize)) + (x as usize) * (OUTPUT_BPP as usize);
            output.write_all(&[pixels[start + 2], pixels[start + 1], pixels[start]]).unwrap();
        }
    }
}

