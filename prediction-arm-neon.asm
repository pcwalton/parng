@ parng/prediction-arm-neon.asm
@
@ Copyright (c) 2016 Mozilla Foundation

.global parng_predict_scanline_none_packed_32bpp
.global parng_predict_scanline_none_strided_32bpp
.global parng_predict_scanline_none_packed_24bpp
.global parng_predict_scanline_none_strided_24bpp
.global parng_predict_scanline_none_packed_16bpp
.global parng_predict_scanline_none_packed_8bpp
.global parng_predict_scanline_left_packed_32bpp
.global parng_predict_scanline_left_strided_32bpp
.global parng_predict_scanline_left_packed_24bpp
.global parng_predict_scanline_left_strided_24bpp
.global parng_predict_scanline_left_packed_16bpp
.global parng_predict_scanline_left_packed_8bpp
.global parng_predict_scanline_up_packed_32bpp
.global parng_predict_scanline_up_strided_32bpp
.global parng_predict_scanline_up_packed_24bpp
.global parng_predict_scanline_up_strided_24bpp
.global parng_predict_scanline_up_packed_16bpp
.global parng_predict_scanline_up_packed_8bpp
.global parng_predict_scanline_average_strided_32bpp
.global parng_predict_scanline_average_strided_24bpp
.global parng_predict_scanline_paeth_strided_32bpp
.global parng_predict_scanline_paeth_strided_24bpp

#define dest r0
#define src r1
#define prev r2
#define length r3
#define stride r4

.section text

@ Helper functions to factor out the unsafe memory accesses in one place follow.

.macro prolog
    stmfd sp!,{r4-r6}
    ldr stride,[sp,#5*4]
.endm

.macro loop_start
    prolog
    mov r5,#0
1:
.endm

.macro loop_end dest_stride, src_stride
    add r5,r5,#\dest_stride
    add r6,r6,#\src_stride
    cmp r5,length
    blo 1b
.endm

.macro loop_end_stride stride
    add r5,r5,#\stride
    cmp r5,length
    blo 1b
.endm

.macro epilog
    ldmfd sp!,{r4-r6,pc}
.endm

@ We factor out this safe pattern for the benefit of the static analysis, which would interpret
@ `[0]` as an unsafe memory access.
.macro move_neon_byte_to_register dest,src
    vmov.u8 \dest,\src[0]
.endm

@ #begin-safe-code

@ load_8bpp_to_32bpp_table_lookup_mask(r32 dest_hi, r32 dest_lo)
@
@ Register clobbers: r7
.macro load_8bpp_to_32bpp_table_lookup_mask dest_hi, dest_lo
    ldr r7,=0x01010101
    vmov.u8 \dest_hi,r7
    mov r7,#0
    vmov.u8 \dest_lo,r7
.endm

@ load_16bpp_to_32bpp_table_lookup_mask(r32 dest_hi, r32 dest_lo)
@
@ Register clobbers: r7
.macro load_16bpp_to_32bpp_table_lookup_mask dest_hi, dest_lo
    ldr r7,=0xffff0302
    vmov.u8 \dest_hi,r7
    ldr r7,=0xffff0100
    vmov.u8 \dest_lo,r7
.endm

@ load_24bpp_to_32bpp_table_lookup_mask(r32 dest_hi, r32 dest_lo)
@
@ Register clobbers: r7
.macro load_24bpp_to_32bpp_table_lookup_mask dest_hi, dest_lo
    ldr r7,=0xff050403
    vmov.u8 \dest_hi,r7
    ldr r7,=0xff020100
    vmov.u8 \dest_lo,r7
.endm

@ load_32bpp_opaque_alpha_mask(r32 dest_hi, r32 dest_lo)
@
@ Register clobbers: r7
.macro load_32bpp_opaque_alpha_mask dest_hi, dest_lo
    mov r7,#0xff000000
    vmov.u8 \dest_hi,r7
    vmov.u8 \dest_lo,r7
.endm

@ parng_predict_scanline_none_packed_32bpp(uint8x4 *dest,
@                                          uint8x4 *src,
@                                          uint8x4 *prev,
@                                          uint32_t length,
@                                          uint32_t stride)
parng_predict_scanline_none_packed_32bpp:
    prolog
    loop_start
    vldr d0,[src]
    vstr d0,[dest]
    loop_end 8,8
    epilog

@ parng_predict_scanline_none_strided_32bpp(uint8x4 *dest,
@                                           uint8x4 *src,
@                                           uint8x4 *prev,
@                                           uint32_t length,
@                                           uint32_t stride)
parng_predict_scanline_none_strided_32bpp:
    prolog
    loop_start
    ldr r7,[src]
    str r7,[src]
    loop_end_stride 4
    epilog

@ parng_predict_scanline_none_packed_24bpp(uint8x4 *dest,
@                                          uint8x4 *src,
@                                          uint8x4 *prev,
@                                          uint32_t length,
@                                          uint32_t stride)
parng_predict_scanline_none_packed_24bpp:
    prolog
    loop_start
    load_24bpp_to_32bpp_table_lookup_mask s3,s2
    vldr d0,[src]
    vtbl.8 d0,{d2,d3},d0
    vstr d0,[dest]
    loop_end 8,6
    epilog

@ parng_predict_scanline_none_strided_24bpp(uint8x4 *dest,
@                                           uint8x4 *src,
@                                           uint8x4 *prev,
@                                           uint32_t length,
@                                           uint32_t stride)
parng_predict_scanline_none_strided_24bpp:
    prolog
    loop_start
    ldr r7,[src]
    bic r7,#0xff000000
    str r7,[dest]
    loop_end_stride 4
    epilog

@ parng_predict_scanline_none_packed_16bpp(uint8x4 *dest,
@                                          uint8x4 *src,
@                                          uint8x4 *prev,
@                                          uint32_t length,
@                                          uint32_t stride)
parng_predict_scanline_none_packed_16bpp:
    prolog
    load_16bpp_to_32bpp_table_lookup_mask s3,s2
    loop_start
    vldr d0,[src]
    vtbl.8 d0,{d1},d0
    vstr d0,[dest]
    loop_end 8,4
    epilog

@ parng_predict_scanline_none_packed_8bpp(uint8x4 *dest,
@                                         uint8x4 *src,
@                                         uint8x4 *prev,
@                                         uint32_t length,
@                                         uint32_t stride)
parng_predict_scanline_none_packed_8bpp:
    prolog
    load_8bpp_to_32bpp_table_lookup_mask s3,s2
    loop_start
    vld1.16 {d0},[src]
    vtbl.8 d0,{d1},d0
    vst1.32 {d0},[dest]
    loop_end 8,2
    epilog

@ predict_pixels_left_4()
@
@ Register inputs: {d0,d1} = [ 0, 0, 0, z ]
@                  {d2,d3} = src
@                  {d4,d5} = [ 0, 0, 0, 0 ]
@ Register outputs: {d2-d3} = result
@ Register clobbers: None
.macro predict_pixels_left_4
    vzip.32 q1,q2           @ d5 = [ 0, d ], d4 = [ 0, c ], d3 = [ 0, b ], d2 = [ 0, a ]
    vadd.u8 q1,q1,q2        @ d5 = [ 0, b+d ], d4 = [ 0, a+c ], d3 = [ 0, b ], d2 = [ 0, a ]
    vadd.u8 d2,d1,d2        @ d2 = [ 0, a+z ]
    vadd.u8 d3,d2,d3        @ d3 = [ 0, a+b+z ]
    vadd.u8 d4,d3,d4        @ d4 = [ 0, a+b+c+z ]
    vadd.u8 d5,d4,d5        @ d5 = [ 0, a+b+c+d+z ]
    vuzp.32 q1,q2           @ d5 = d4 = [ 0 ], d3 = [ a+b+c+d+z, a+b+c+z ], d2 = [ a+b+z, a+z ]
    vsri.64 d1,d3,#32       @ d1 = [ 0, a+b+c+d+z ]
.endm

@ predict_pixels_left_2()
@
@ Register inputs: d0 = [ 0, z ]
@                  d1 = src
@                  d2 = [ 0, 0 ]
@ Register outputs: d1 = result
@ Register clobbers: None
.macro predict_pixels_left_2
    vzip.32 d1,d2           @ d2 = [ 0, b ], d1 = [ 0, a ]
    vadd.u8 d1,d1,d0        @ d2 = [ 0, b ], d1 = [ 0, a+z ]
    vadd.u8 d2,d1,d2        @ d2 = [ 0, a+b+z ], d1 = [ 0, a+z ]
    vuzp.32 d1,d2           @ d2 = [ 0 ], d1 = [ a+b+z, a+z ]
    vsri.32 d0,d1,#32       @ d0 = [ 0, a+b+z ]
.endm

@ parng_predict_scanline_left_packed_32bpp(uint8x4 *dest,
@                                          uint8x4 *src,
@                                          uint8x4 *prev,
@                                          uint32_t length,
@                                          uint32_t stride)
parng_predict_scanline_left_packed_32bpp:
    prolog
    veor.u8 q0,q0,q0
    veor.u8 q2,q2,q2
    loop_start
    predict_pixels_left_4
    loop_end 16,16
    epilog

@ parng_predict_scanline_left_strided_32bpp(uint8x4 *dest,
@                                           uint8x4 *src,
@                                           uint8x4 *prev,
@                                           uint32_t length,
@                                           uint32_t stride)
parng_predict_scanline_left_strided_32bpp:
    prolog
    veor.u8 d0,d0,d0
    loop_start
    vld1.32 {d1},[src]
    vadd.u8 d0,d0,d1
    vst1.32 {d0},[dest]
    loop_end_stride 4
    epilog

@ parng_predict_scanline_left_packed_24bpp(uint8x4 *dest,
@                                          uint8x3 *src,
@                                          uint8x4 *prev,
@                                          uint32_t length,
@                                          uint32_t stride)
parng_predict_scanline_left_packed_24bpp:
    prolog
    load_24bpp_to_32bpp_table_lookup_mask s7,s6
    load_32bpp_opaque_alpha_mask s9,s8
    veor.u8 d0,d0,d0        @ d0 = [ 0 ]
    loop_start
    vldr d1,[src]           @ d1 = src (24bpp)
    vtbl.8 d1,{d3},d1       @ d1 = [ b, a ]
    predict_pixels_left_2
    vorr.u8 d1,d4,d1        @ d1 = result with alpha == 0xff
    vstr d1,[dest]          @ write pixels
    loop_end 8,6
    epilog

@ parng_predict_scanline_left_strided_24bpp(uint8x4 *dest,
@                                           uint8x3 *src,
@                                           uint8x4 *prev,
@                                           uint32_t length,
@                                           uint32_t stride)
@
@ TODO(pcwalton): This could save a cycle or two by leaving 0xff somewhere in d1 and having the
@ mask fetch it.
parng_predict_scanline_left_strided_24bpp:
    prolog
    load_24bpp_to_32bpp_table_lookup_mask s7,s6
    load_32bpp_opaque_alpha_mask s9,s8
    veor.u8 d0,d0,d0        @ d0 = [ 0 ]
    loop_start
    vld1.32 {d1},[src]      @ d1 = src (24bpp)
    vtbl.8 d1,{d3},d1       @ d1 = src (32bpp)
    vadd.u8 d0,d0,d1        @ d0 = result
    vorr.u8 d0,d0,d4        @ d0 = result with alpha == 0xff
    vst1.32 {d0},[dest]
    loop_end_stride 4
    epilog

@ parng_predict_scanline_left_packed_16bpp(uint8x4 *dest,
@                                          uint8x3 *src,
@                                          uint8x4 *prev,
@                                          uint32_t length,
@                                          uint32_t stride)
parng_predict_scanline_left_packed_16bpp:
    prolog
    load_16bpp_to_32bpp_table_lookup_mask s15,s14
    veor.u8 q0,q0,q0        @ d1 = [ 0 ], d0 = [ 0 ]
    veor.u8 d2,d2,d2        @ d2 = [ 0 ]
    loop_start
    vld1.32 {d0},[src]      @ d0 = src (16bpp)
    vtbl.8 d0,{d7},d0       @ d0 = [ 0, 0, b, a ]
    predict_pixels_left_2
    vstr d1,[dest]
    loop_end 8,4
    epilog

@ parng_predict_scanline_left_packed_8bpp(uint8x4 *dest,
@                                         uint8x3 *src,
@                                         uint8x4 *prev,
@                                         uint32_t length,
@                                         uint32_t stride)
parng_predict_scanline_left_packed_8bpp:
    prolog
    load_8bpp_to_32bpp_table_lookup_mask s15,s14
    veor.u8 q0,q0,q0        @ d1 = [ 0 ], d0 = [ 0 ]
    veor.u8 d2,d2,d2        @ d2 = [ 0 ]
    loop_start
    vld1.16 d1,[src]        @ d1 = src (8bpp)
    vtbl.8 d1,{d7},d1       @ d1 = [ 0, 0, b, a ]
    predict_pixels_left_2
    vstr d1,[dest]
    loop_end 8,2
    epilog

@ parng_predict_scanline_up_packed_32bpp(uint8x4 *dest,
@                                        uint8x4 *src,
@                                        uint8x4 *prev,
@                                        uint32_t length,
@                                        uint32_t stride)
parng_predict_scanline_up_packed_32bpp:
    prolog
    loop_start
    vldr d0,[src]
    vldr d1,[prev]
    vadd.u8 d0,d0,d1
    vstr d0,[dest]
    loop_end 8,8
    epilog

@ parng_predict_scanline_up_strided_32bpp(uint8x4 *dest,
@                                         uint8x4 *src,
@                                         uint8x4 *prev,
@                                         uint32_t length,
@                                         uint32_t stride)
parng_predict_scanline_up_strided_32bpp:
    prolog
    loop_start
    vld1.32 {d0},[prev]
    vld1.32 {d1},[src]
    vadd.u8 d0,d0,d1
    vst1.32 {d0},[dest]
    loop_end_stride 4
    epilog

@ parng_predict_scanline_up_packed_24bpp(uint8x4 *dest,
@                                        uint8x3 *src,
@                                        uint8x4 *prev,
@                                        uint32_t length,
@                                        uint32_t stride)
@
@ There is no need to make the alpha opaque here as long as the previous scanline had opaque alpha.
parng_predict_scanline_up_packed_24bpp:
    prolog
    load_24bpp_to_32bpp_table_lookup_mask s15,s14
    loop_start
    vldr d0,[prev]          @ d0 = prev
    vldr d1,[src]           @ d1 = src (24bpp)
    vtbl.8 d1,{d7},d1       @ d1 = src (32bpp)
    vadd.u8 d0,d0,d1        @ d0 = prev + src
    vstr d0,[dest]          @ write result
    loop_end 8,6
    epilog

@ parng_predict_scanline_up_strided_24bpp(uint8x4 *dest,
@                                         uint8x3 *src,
@                                         uint8x4 *prev,
@                                         uint32_t length,
@                                         uint32_t stride)
@
@ There is no need to make the alpha opaque here as long as the previous scanline had opaque alpha.
parng_predict_scanline_up_strided_24bpp:
    prolog
    load_24bpp_to_32bpp_table_lookup_mask s15,s14
    loop_start
    vld1.32 {d0},[prev]     @ d0 = prev
    vld1.32 {d1},[src]      @ d1 = src
    vadd.u8 d0,d0,d1        @ d1 = prev + src (24bpp)
    vtbl.8 d0,{d7},d1       @ d0 = prev + src (32bpp)
    vst1.32 {d0},[dest]     @ write result
    loop_end_stride 3
    epilog

@ parng_predict_scanline_up_packed_16bpp(uint8x4 *dest,
@                                        uint8x2 *src,
@                                        uint8x4 *prev,
@                                        uint32_t length,
@                                        uint32_t stride)
parng_predict_scanline_up_packed_16bpp:
    prolog
    load_16bpp_to_32bpp_table_lookup_mask s15,s14
    loop_start
    vld1.32 {d0},[prev]     @ d0 = prev
    vld1.32 {d1},[src]      @ d1 = src (16bpp)
    vtbl.8 d1,{d7},d1       @ d1 = src (32bpp)
    vadd.u8 d0,d0,d1        @ d0 = prev + src
    vstr d0,[dest]
    loop_end 8,4
    epilog

@ parng_predict_scanline_up_packed_8bpp(uint8x4 *dest,
@                                       uint8 *src,
@                                       uint8x4 *prev,
@                                       uint32_t length,
@                                       uint32_t stride)
parng_predict_scanline_up_packed_8bpp:
    prolog
    load_8bpp_to_32bpp_table_lookup_mask s15,s14
    loop_start
    vldr d0,[prev]          @ d0 = prev
    vld1.16 d1,[src]        @ d1 = src (8bpp)
    vtbl.8 d0,{d7},d0       @ d1 = src (32bpp)
    vadd.u8 d0,d0,d1        @ d0 = prev + src
    vstr d0,[dest]          @ write result
    loop_end 8,2
    epilog

@ parng_predict_scanline_average_strided_32bpp(uint8x4 *dest,
@                                              uint8x4 *src,
@                                              uint8x4 *prev,
@                                              uint32_t length,
@                                              uint32_t stride)
parng_predict_scanline_average_strided_32bpp:
    prolog
    veor.u8 d0,d0,d0        @ d0 = [ 0 ]
    loop_start
    vld1.32 {d1},[prev]     @ d1 = prev
    vhadd.u8 d0,d0,d1       @ d1 = avg(a, b)
    vld1.32 {d1},[src]      @ d1 = src
    vadd.u8 d0,d0,d1        @ d0 = src + avg(a, b)
    vst1.32 {d0},[dest]
    loop_end_stride 4
    epilog

@ parng_predict_scanline_average_strided_24bpp(uint8x4 *dest,
@                                              uint8x3 *src,
@                                              uint8x4 *prev,
@                                              uint32_t length,
@                                              uint32_t stride)
@
@ There is no need to make the alpha opaque here as long as the previous scanline had opaque alpha.
parng_predict_scanline_average_strided_24bpp:
    prolog
    vmov.i32 d2,#0xff000000 @ d2 = 0xff000000
    loop_start
    vld1.32 {d1},[prev]     @ d1 = prev
    vhadd.u8 d0,d0,d1       @ d1 = avg(a, b)
    vld1.32 {d1},[src]      @ d1 = src
    vorr.u8 d1,d1,d2        @ d1 = src (opaque alpha)
    vadd.u8 d0,d0,d1        @ d0 = src + avg(a, b)
    vst1.32 {d0},[dest]
    loop_end_stride 3
    epilog

@ Register inputs: d0 = a, d1 = prev (8-bit), d2 = c, d3 = 0
@ Register outputs: d0 = dest (new a), d2 = b (new c)
@ Register clobbers: d4, d5, d6, d7
.macro predict_pixels_paeth
    vzip.8 d1,d3            @ d1 = b (16-bit)@ d3 = 0
    vsub.s16 d4,d2,d1       @ d4 = c - b = ±pa
    vsub.s16 d5,d0,d2       @ d5 = a - c = ±pb
    vabd.s16 d6,d5,d4       @ d6 = |a - c - c + b| = |a + b - 2c| = pc
    vabs.s16 q2,q2          @ d4 = pa, d5 = pb
    vmin.s16 d7,d4,d5       @ d7 = min(pa, pb)
    vcgt.s16 d4,d4,d5       @ d4 = pa > pb = ¬(pa ≤ pb)
    vcgt.s16 d7,d7,d6       @ d7 = min(pa, pb) > pc = ¬(pa ≤ pc) ∧ ¬(pb ≤ pc)
    vbic.u16 d5,d0,d4       @ d5 = a if pa ≤ pb
    vand.u16 d4,d4,d1       @ d4 = b if ¬(pa ≤ pb)
    vorr.u16 d4,d4,d7       @ d4 = ¬(pa ≤ pc) ∧ ¬(pb ≤ pc) ? TRUE : ¬(pa ≤ pb) ? b : FALSE
    vorr.u16 d5,d5,d4       @ d5 = ¬(pa ≤ pc) ∧ ¬(pb ≤ pc) ? TRUE : pa ≤ pb ? a : b
    vand.u16 d4,d4,d2       @ d7 = ¬(pa ≤ pc) ∧ ¬(pb ≤ pc) ? c : ¬(pa ≤ pb) ? undef : FALSE
    vmax.s16 d4,d5,d4       @ d4 = ¬(pa ≤ pc) ∧ ¬(pb ≤ pc) ? c : (pa ≤ pb) ∧ (pa ≤ pc) ? a : b
    vld1.32 {d0},[src]      @ d0 = original pixel (8-bit)
    vzip.8 d0,d3            @ d0 = original pixel (16-bit)@ d3 = 0
    vadd.u8 d0,d4           @ d0 = next a = output pixel
    vmov.u16 d4,d0          @ d4 = output pixel (16-bit)
    vuzp.8 d0,d3            @ d0 = output pixel (8-bit)@ d3 = 0
    vmov.u16 d2,d1          @ c = b
.endm

@ parng_predict_scanline_paeth_strided_32bpp(uint8x4 *dest,
@                                            uint8x4 *src,
@                                            uint8x4 *prev,
@                                            uint32_t length,
@                                            uint32_t stride)
parng_predict_scanline_paeth_strided_32bpp:
    prolog
    veor.u8 d0,d0,d0            @ d0 = [ 0 ]
    vmov.u8 d2,d0               @ d2 = [ 0 ]
    vmov.u8 d3,d0               @ d3 = [ 0 ]
    loop_start
    vld1.32 {d1},[prev]         @ d1 = prev (8-bit)
    predict_pixels_paeth        @ d0 = result
    vst1.32 {d0},[dest]         @ write result
    loop_end_stride 4
    epilog

@ parng_predict_scanline_paeth_strided_24bpp(uint8x4 *dest,
@                                            uint8x3 *src,
@                                            uint8x4 *prev,
@                                            uint32_t length,
@                                            uint32_t stride)
parng_predict_scanline_paeth_strided_24bpp:
    prolog
    veor.u8 d0,d0,d0            @ d0 = [ 0 ]
    vmov.u8 d2,d0               @ d2 = [ 0 ]
    vmov.u8 d3,d0               @ d3 = [ 0 ]
    loop_start
    vld1.32 {d1},[prev]         @ d1 = prev (8-bit)
    predict_pixels_paeth        @ d0 = result
    move_neon_byte_to_register r7,d0
    orr r7,r7,#0xff000000
    str r7,[dest]               @ write result
    loop_end_stride 3
    epilog

