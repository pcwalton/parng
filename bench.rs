// parng/bench.rs
//
// Any copyright is dedicated to the Public Domain.
// http://creativecommons.org/publicdomain/zero/1.0/

#[cfg(feature = "bench")]
mod bench {
    extern crate stb_image;
    extern crate time;

    use std::env;

    const RUNS: u32 = 10;

    pub fn go() {
        let input_path = env::args().skip(1).next().unwrap();

        let mut total_elapsed_time = 0;
        for _ in 0..RUNS {
            let before = time::precise_time_ns();
            stb_image::image::load(&input_path);
            let elapsed = time::precise_time_ns() - before;
            total_elapsed_time += elapsed;
            println!("Elapsed time: {}ms", elapsed as f32 / 1_000_000.0);
        }

        total_elapsed_time /= RUNS as u64;
        println!("Mean elapsed time: {}ms", total_elapsed_time as f32 / 1_000_000.0);
    }
}

#[cfg(feature = "bench")]
fn main() {
    bench::go()
}

#[cfg(not(feature = "bench"))]
fn main() {
    println!("Compile with the `bench` feature to use the benchmarking tool.");
}

