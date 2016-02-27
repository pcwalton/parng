# `parng`

**`parng` is not ready yet. It still has serious bugs and needs benchmarks and a test suite. The
following statements are likely lies. Please do not publicize it widely until ready.**

## Introduction

`parng` is a advanced, parallel PNG decoder written in Rust. It has several features that make it
especially suitable for client software:

* Very fast.

  - `parng` combines both multicore parallelism (taking advantage of dual core if available) and
    SIMD parallelism (with AVX on x86-64 and NEON on ARM if available).

  - In all benchmarks against popular PNG decoding libraries, `parng` is the fastest PNG decoder by
    a significant margin, reaching speedups of up to 2x over the next fastest library.

  - The AVX SIMD routines were generated using the Stanford STOKE superoptimizer and manually
    refactored for clarity.

* Secure.

  - `parng` is written almost entirely in safe Rust and memory-safety-verified assembly.

  - The implementation uses no unsafe code other than the system libraries (including `zlib`), a
	tiny driver for the accelerated assembly code, and a helper routine that avoids zeroing memory
    (but see below).

  - The SIMD assembly code is verified for memory safety using a custom static analysis written in
    Ruby.

  - `parng` can be made to zero out buffers if desired with a Cargo feature (`zero-out-buffers`).

* Flexible.

  - `parng` has a C API, so it can be used from C, C++, or any language that can call out to C.

  - The input and output are completely customizable. With the data provider API, you can store and
    the image data however you like and display the image whenever you wish.

  - `parng` has a scalar fallback for systems that do not support AVX or NEON.

## License

`parng` is licensed under the same terms as Rust itself. See the `LICENSE-APACHE` and
`LICENSE-MIT` files.

## Acknowledgements

* The prefix sum technique used to implement the Left filter is taken from
https://github.com/kobalicek/simdtests/blob/master/depng/depng_sse2.cpp -- thanks!

