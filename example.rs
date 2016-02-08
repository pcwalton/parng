//! An example showing use of `parng`.

extern crate byteorder;
extern crate clap;
extern crate parng;
extern crate time;

use byteorder::{LittleEndian, WriteBytesExt};
use clap::{App, Arg};
use parng::{AddDataResult, DecodeResult, Image};
use std::fs::File;
use std::io::{BufWriter, Cursor, Read, Write};

const BPP: u32 = 4;

fn main() {
    let matches = App::new("example").arg(Arg::with_name("INPUT").required(true))
                                     .arg(Arg::with_name("OUTPUT").required(true))
                                     .get_matches();

    let in_path = matches.value_of("INPUT").unwrap();
    let out_path = matches.value_of("OUTPUT").unwrap();

    let mut input = File::open(in_path).unwrap();
    let mut input_buffer = vec![];
    input.read_to_end(&mut input_buffer).unwrap();

    let mut image = Image::new().unwrap();
    let mut input_buffer = Cursor::new(&input_buffer[..]);
    while let AddDataResult::Continue = image.add_data(&mut input_buffer).unwrap() {}

    let dimensions = image.metadata().as_ref().unwrap().dimensions;

    let before = time::precise_time_ns();
    let mut pixels = vec![];
    for y in 0..dimensions.height {
        loop {
            while let AddDataResult::Continue = image.add_data(&mut input_buffer).unwrap() {}
            match image.decode().unwrap() {
                DecodeResult::Scanline(scanline) => {
                    pixels.extend_from_slice(&scanline[..]);
                    break
                }
                DecodeResult::None => {}
            }
        }
    }
    println!("Elapsed time: {}ms", (time::precise_time_ns() - before) as f32 / 1_000_000.0);

    let mut output = BufWriter::new(File::create(out_path).unwrap());
    output.write_all(&[0, 0, 2, 0,
                       0, 0, 0, 0,
                       0, 0, 0, 0]).unwrap();
    output.write_u16::<LittleEndian>(dimensions.width as u16).unwrap();
    output.write_u16::<LittleEndian>(dimensions.height as u16).unwrap();
    output.write_all(&[24, 0]).unwrap();

    for y in 0..dimensions.height {
        let y = dimensions.height - y - 1;
        for x in 0..dimensions.width {
            let start = (((y * dimensions.width) + x) * BPP) as usize;
            output.write_all(&[pixels[start + 2], pixels[start + 1], pixels[start]]).unwrap();
        }
    }
}

