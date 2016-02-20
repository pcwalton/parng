//! An example showing use of `parng`.

extern crate byteorder;
extern crate parng;
extern crate time;

use byteorder::{LittleEndian, WriteBytesExt};
use parng::simple::Image;
use std::env;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::process;

const OUTPUT_BPP: u32 = 4;

fn usage() -> ! {
    write!(io::stderr(), "usage: example image.png image.tga");
    process::exit(0)
}

fn main() {
    let mut args = env::args().skip(1);
    let in_path = args.next().unwrap_or_else(|| usage());
    let out_path = args.next().unwrap_or_else(|| usage());

    let before = time::precise_time_ns();
    let image = Image::load(&mut File::open(in_path).unwrap()).unwrap();
    let elapsed = time::precise_time_ns() - before;
    println!("Elapsed time: {}ms", elapsed as f32 / 1_000_000.0);

    let mut output = BufWriter::new(File::create(out_path).unwrap());
    output.write_all(&[0, 0, 2, 0,
                       0, 0, 0, 0,
                       0, 0, 0, 0]).unwrap();
    output.write_u16::<LittleEndian>(image.width as u16).unwrap();
    output.write_u16::<LittleEndian>(image.height as u16).unwrap();
    output.write_all(&[24, 0]).unwrap();

    for y in 0..image.height {
        let y = image.height - y - 1;
        for x in 0..image.width {
            let start = (image.stride * (y as usize)) + (x as usize) * (OUTPUT_BPP as usize);
            output.write_all(&[
                image.pixels[start + 2],
                image.pixels[start + 1],
                image.pixels[start + 0],
            ]).unwrap();
        }
    }
}

