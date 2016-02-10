//! An example showing use of `parng`.

extern crate byteorder;
extern crate clap;
extern crate parng;
extern crate time;

use byteorder::{LittleEndian, WriteBytesExt};
use clap::{App, Arg};
use parng::interlacing::{self, Adam7Scanlines, LevelOfDetail};
use parng::{AddDataResult, DecodeResult, Image};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::ptr;

const OUTPUT_BPP: u32 = 4;

fn main() {
    let matches = App::new("example").arg(Arg::with_name("INPUT").required(true))
                                     .arg(Arg::with_name("OUTPUT").required(true))
                                     .get_matches();

    let in_path = matches.value_of("INPUT").unwrap();
    let out_path = matches.value_of("OUTPUT").unwrap();

    let mut input = File::open(in_path).unwrap();
    let mut image = Image::new().unwrap();
    while let AddDataResult::Continue = image.add_data(&mut input).unwrap() {}

    let dimensions = image.metadata().as_ref().unwrap().dimensions;
    let color_depth = image.metadata().as_ref().unwrap().color_depth;

    let before = time::precise_time_ns();

    let mut pixels = vec![];
    let mut interlaced_pixels = [vec![], vec![], vec![], vec![], vec![], vec![], vec![]];
    let stride = dimensions.width as usize * 4;
    pixels.reserve((dimensions.height as usize) * stride);

    let mut y = 0;
    while y < dimensions.height {
        while let AddDataResult::Continue = image.add_data(&mut input).unwrap() {}
        match image.decode().unwrap() {
            DecodeResult::Scanline(scanline, LevelOfDetail::Adam7(lod)) => {
                //println!("got scanline with LOD {:?}", lod);
                interlaced_pixels[lod as usize].extend_from_slice(&scanline[..]);
                if lod == 6 && interlaced_pixels[6].len() >= stride * 4 {
                    y += 8;

                    // FIXME(pcwalton): Do this in a separate thread for parallelism.
                    //
                    // TODO(pcwalton): Figure out a nice way to expose this in the API. The
                    // low-level control philosophy of parng has jumped the shark at this
                    // point.
                    let original_length = pixels.len();
                    pixels.resize(original_length + stride * 8, 0);
                    let lengths = [
                        stride / 8,
                        stride / 8,
                        stride / 4,
                        stride / 2,
                        stride,
                        stride * 2,
                        stride * 4
                    ];
                    if interlaced_pixels.iter().zip(lengths.iter()).all(|(pixels, &length)| {
                                pixels.len() >= length
                            }) {
                        {
                            let scanlines = Adam7Scanlines {
                                lod0: [&interlaced_pixels[0][..]],
                                lod1: Some([&interlaced_pixels[1][..]]),
                                lod2: Some([&interlaced_pixels[2][..]]),
                                lod3: Some([
                                    &interlaced_pixels[3][0..(stride / 2)],
                                    &interlaced_pixels[3][(stride / 2)..stride],
                                ]),
                                lod4: Some([
                                    &interlaced_pixels[4][0..(stride / 2)],
                                    &interlaced_pixels[4][(stride / 2)..stride],
                                ]),
                                lod5: Some([
                                    &interlaced_pixels[5][0..(stride / 2)],
                                    &interlaced_pixels[5][(stride / 2)..stride],
                                    &interlaced_pixels[5][stride..(stride * 3 / 2)],
                                    &interlaced_pixels[5][(stride * 3 / 2)..],
                                ]),
                                lod6: Some([
                                    &interlaced_pixels[6][0..stride],
                                    &interlaced_pixels[6][stride..stride * 2],
                                    &interlaced_pixels[6][stride * 2..stride * 3],
                                    &interlaced_pixels[6][stride * 3..],
                                ]),
                            };

                            interlacing::deinterlace_adam7(&mut pixels[original_length..],
                                                           &scanlines,
                                                           dimensions.width,
                                                           color_depth);
                        }

                        // FIXME(pcwalton): Terrible.
                        for (buffer, &length) in interlaced_pixels.iter_mut().zip(lengths.iter()) {
                            let original_length = buffer.len();
                            unsafe {
                                ptr::copy(buffer.as_mut_ptr().offset(length as isize),
                                          buffer.as_mut_ptr(),
                                          original_length - length);
                            }
                            buffer.truncate(original_length - length);
                        }
                    } else {
                        println!("bad lengths: {:?} stride={}",
                                 interlaced_pixels.iter()
                                                  .map(|pixels| pixels.len())
                                                  .collect::<Vec<_>>(),
                                 stride);
                    }
                }
            }
            DecodeResult::Scanline(scanline, LevelOfDetail::None) => {
                y += 1;
                pixels.extend_from_slice(&scanline[..]);
            }
            DecodeResult::None => {}
        }
    }
    pixels.resize((dimensions.height as usize) * stride, 0);   // FIXME(pcwalton)
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
            let start = (((y * dimensions.width) + x) * OUTPUT_BPP) as usize;
            output.write_all(&[pixels[start + 2], pixels[start + 1], pixels[start]]).unwrap();
        }
    }
}

