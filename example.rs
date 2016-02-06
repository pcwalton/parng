//! An example showing use of `parng`.

extern crate byteorder;
extern crate clap;
extern crate parng;
extern crate rayon;
extern crate time;

use byteorder::{LittleEndian, WriteBytesExt};
use clap::{App, Arg};
use parng::{AddDataResult, Image};
use rayon::Configuration;
use std::fs::File;
use std::io::{BufWriter, Cursor, Read, Write};
use std::str::FromStr;

const BPP: u32 = 4;

fn main() {
    let matches = App::new("example").arg(Arg::with_name("INPUT").required(true))
                                     .arg(Arg::with_name("OUTPUT").required(true))
                                     .arg(Arg::with_name("t").short("t")
                                                             .value_name("THREADS")
                                                             .takes_value(true))
                                     .get_matches();

    let in_path = matches.value_of("INPUT").unwrap();
    let out_path = matches.value_of("OUTPUT").unwrap();
    let thread_count = matches.value_of("t").map(FromStr::from_str);

    let mut input = File::open(in_path).unwrap();
    let mut input_buffer = vec![];
    input.read_to_end(&mut input_buffer).unwrap();

    let before = time::precise_time_ns();
    let mut image = Image::new().unwrap();
    image.preallocate_space(4000, 4000, 4);
    while let AddDataResult::Continue =
        image.add_data(&mut Cursor::new(&input_buffer[..])).unwrap() {}
    println!("Entropy decoding: {}ms", (time::precise_time_ns() - before) as f32 / 1_000_000.0);

    let mut config = Configuration::new();
    if let Some(Ok(thread_count)) = thread_count {
        config = config.set_num_threads(thread_count)
    }
    rayon::initialize(config).unwrap();

    let before = time::precise_time_ns();
    let mut pixels = vec![];
    image.decode(&mut pixels);
    println!("Prediction: {}ms", (time::precise_time_ns() - before) as f32 / 1_000_000.0);

    let metadata = image.metadata().as_ref().unwrap();
    let mut output = BufWriter::new(File::create(out_path).unwrap());
    output.write_all(&[0, 0, 2, 0,
                       0, 0, 0, 0,
                       0, 0, 0, 0]).unwrap();
    output.write_u16::<LittleEndian>(metadata.dimensions.width as u16).unwrap();
    output.write_u16::<LittleEndian>(metadata.dimensions.height as u16).unwrap();
    output.write_all(&[24, 0]).unwrap();

    for y in 0..metadata.dimensions.height {
        let y = metadata.dimensions.height - y - 1;
        for x in 0..metadata.dimensions.width {
            let start = (((y * metadata.dimensions.width) + x) * BPP) as usize;
            output.write_all(&[pixels[start + 2], pixels[start + 1], pixels[start]]).unwrap();
        }
    }
}

