//! An example showing use of `parng`.

extern crate parng;

use parng::Image;
use std::fs::File;
use std::io::{self, BufReader, Write};
use std::process;

fn usage() -> String {
    writeln!(&mut io::stderr(), "usage: example image.png image.tga");
    process::exit(0);
}

fn main() {
    let mut args = std::env::args();
    drop(args.next().unwrap());
    let in_path = args.next().unwrap_or_else(usage);
    let out_path = args.next().unwrap_or_else(usage);

    let input = File::open(in_path).unwrap();
    let input_length = input.metadata().unwrap().len();
    let input = BufReader::new(input);
    let mut image = Image::new(input);
    image.load_metadata().unwrap();
    image.start_decoding().unwrap();
    image.buffer_data(input_length as u32).unwrap();
}

