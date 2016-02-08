// parng/build.rs

extern crate gcc;

use gcc::Config;
use std::env;
use std::process::Command;

fn nasm(out_path: &str, in_path: &str) {
    Command::new(&format!("nasm")).arg("-f").arg("macho64")
                                  .arg("--prefix").arg("_")
                                  .arg("-o").arg(out_path)
                                  .arg(in_path)
                                  .status()
                                  .unwrap();
}

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();

    let interlace_o = format!("{}/interlace.o", out_dir);
    let predict_o = format!("{}/predict.o", out_dir);
    nasm(&interlace_o, "interlace.asm");
    nasm(&predict_o, "predict.asm");

    Config::new().object(&interlace_o).object(&predict_o).compile("libparngacceleration.a")
}

