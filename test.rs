// parng/test.rs
//
// The following applies to this test file only:
//
// Any copyright is dedicated to the Public Domain.
// http://creativecommons.org/publicdomain/zero/1.0/

use std::process::Command;

#[test]
fn verify_asm() {
    assert!(Command::new("ruby").arg("verify-asm.rb")
                                .arg("--arm")
                                .arg("prediction-arm-neon.asm")
                                .spawn()
                                .unwrap()
                                .wait()
                                .unwrap()
                                .success());
    assert!(Command::new("ruby").arg("verify-asm.rb")
                                .arg("--x86_64")
                                .arg("prediction-x86_64-avx.asm")
                                .spawn()
                                .unwrap()
                                .wait()
                                .unwrap()
                                .success());
}

