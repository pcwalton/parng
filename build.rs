// parng/build.rs

extern crate gcc;

use gcc::Config;
use std::env;
use std::process::Command;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();

    let predict_o = format!("{}/predict.o", out_dir);
    Command::new(&format!("nasm")).arg("-f").arg("macho64")
                                  .arg("--prefix").arg("_")
                                  .arg("-o").arg(&predict_o)
                                  .arg("predict.asm")
                                  .status()
                                  .unwrap();

    Config::new().object(&predict_o).compile("libparngpredict.a")
}

