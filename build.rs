// parng/build.rs

extern crate gcc;

use gcc::Config;
use std::env;
use std::process::Command;

#[cfg(target_os="windows")]
fn nasm(out_path: &str, in_path: &str) {
    Command::new(&format!("nasm")).arg("-f").arg("win64")
                                  .arg("-o").arg(out_path)
                                  .arg(in_path)
                                  .status()
                                  .unwrap();
}

#[cfg(target_os="macos")]
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

    let predict_o = format!("{}/predict.o", out_dir);
    nasm(&predict_o, "predict.asm");

    Config::new().object(&predict_o).compile("libparngacceleration.a")
}

