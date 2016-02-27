// parng/build.rs

extern crate gcc;

use gcc::Config;
use std::env;
use std::process::Command;

#[cfg(target_arch="x86_64")]
static PREDICTION_OBJ: &'static str = "prediction-x86_64-avx.o";
#[cfg(target_arch="x86_64")]
static PREDICTION_SOURCE: &'static str = "prediction-x86_64-avx.asm";
#[cfg(target_arch="arm")]
static PREDICTION_OBJ: &'static str = "prediction-arm-neon.o";
#[cfg(target_arch="arm")]
static PREDICTION_SOURCE: &'static str = "prediction-arm-neon.asm";

#[cfg(all(target_arch="x86_64", target_os="windows"))]
fn assemble(out_path: &str, in_path: &str) {
    Command::new(&format!("nasm")).arg("-f").arg("win64")
                                  .arg("-o").arg(out_path)
                                  .arg(in_path)
                                  .status()
                                  .unwrap();
}

#[cfg(all(target_arch="x86_64", target_os="macos"))]
fn assemble(out_path: &str, in_path: &str) {
    Command::new(&format!("nasm")).arg("-f").arg("macho64")
                                  .arg("--prefix").arg("_")
                                  .arg("-o").arg(out_path)
                                  .arg(in_path)
                                  .status()
                                  .unwrap();
}

#[cfg(all(target_arch="arm", target_os="linux"))]
fn assemble(out_path: &str, in_path: &str) {
    let temp_path = format!("{}.s", out_path);
    Command::new(&format!("cpp")).arg("-o").arg(&temp_path)
                                 .arg(in_path)
                                 .status()
                                 .unwrap();
    Command::new(&format!("as")).arg("-o").arg(out_path)
                                .arg("-mfpu=neon")
                                .arg(temp_path)
                                .status()
                                .unwrap();
}

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();

    let predict_o = format!("{}/{}", out_dir, PREDICTION_OBJ);
    assemble(&predict_o, PREDICTION_SOURCE);

    Config::new().object(&predict_o).compile("libparngacceleration.a")
}

