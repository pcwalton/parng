; parng/prediction-arm-neon.asm
;
; Copyright (c) 2016 Mozilla Foundation

; parng_predict_scanline_none_packed_32bpp(uint8x4 *dest,
;                                          uint8x4 *src,
;                                          uint8x4 *prev,
;                                          uint64_t length,
;                                          uint64_t stride)
parng_predict_scanline_none_packed_32bpp:
    prolog
    loop_start
    vld4.32 {d0-d1},[src]
    vst4.32 {d0-d1},[dest]
    loop_end 16,16
    epilog

; parng_predict_scanline_none_strided_32bpp(uint8x4 *dest,
;                                           uint8x4 *src,
;                                           uint8x4 *prev,
;                                           uint64_t length,
;                                           uint64_t stride)
parng_predict_scanline_none_strided_32bpp:
    prolog
    loop_start
    ldr r5,[src]
    str r5,[src]
    loop_end_stride 4
    epilog

; parng_predict_scanline_none_packed_24bpp(uint8x4 *dest,
;                                          uint8x4 *src,
;                                          uint8x4 *prev,
;                                          uint64_t length,
;                                          uint64_t stride)
parng_predict_scanline_none_packed_24bpp:
    prolog
    loop_start
    load_24bpp_to_32bpp_table_lookup_mask d1
    vld4.16 {d0},[src]
    vtbl.8 d0,{d2,d3},d0
    vst4.16 {d0},[dest]
    loop_end 8,6
    epilog

; parng_predict_scanline_none_strided_24bpp(uint8x4 *dest,
;                                           uint8x4 *src,
;                                           uint8x4 *prev,
;                                           uint64_t length,
;                                           uint64_t stride)
parng_predict_scanline_none_strided_24bpp:
    prolog
    loop_start
    ldr r5,[src]
    bic r5,#0xff000000
    str r5,[dest]
    loop_end_stride 4
    epilog

; parng_predict_scanline_none_packed_16bpp(uint8x4 *dest,
;                                          uint8x4 *src,
;                                          uint8x4 *prev,
;                                          uint64_t length,
;                                          uint64_t stride)
parng_predict_scanline_none_packed_16bpp:
    prolog
    load_16bpp_to_32bpp_table_lookup_mask d1
    loop_start
    vld4.8 {d0},[src]
    vtbl.8 d0,d1,d0
    vst4.8 {d0},[dest]
    loop_end 8,4
    epilog

; parng_predict_scanline_none_packed_8bpp(uint8x4 *dest,
;                                         uint8x4 *src,
;                                         uint8x4 *prev,
;                                         uint64_t length,
;                                         uint64_t stride)
parng_predict_scanline_none_packed_8bpp:
    prolog
    load_8bpp_to_32bpp_table_lookup_mask d1
    loop_start
    vld2.8 {d0},[src]
    vtbl.8 d0,d1,d0
    vstr4.8 {d0},[dest]
    loop_end 8,2
    epilog

; predict_pixels_left_4()
;
; Register inputs: {d0,d1} = [ 0, 0, 0, z ]
;                  {d2,d3} = src
;                  {d4,d5} = [ 0, 0, 0, 0 ]
; Register outputs: {d2-d3} = result
; Register clobbers: None
.macro predict_pixels_left_4
    vzip.32 q1,q2           ; d5 = [ 0, d ], d4 = [ 0, c ], d3 = [ 0, b ], d2 = [ 0, a ]
    vadd.u8 q1,q1,q2        ; d5 = [ 0, b+d ], d4 = [ 0, a+c ], d3 = [ 0, b ], d2 = [ 0, a ]
    vadd.u8 d2,d1,d2        ; d2 = [ 0, a+z ]
    vadd.u8 d3,d2,d3        ; d3 = [ 0, a+b+z ]
    vadd.u8 d4,d3,d4        ; d4 = [ 0, a+b+c+z ]
    vadd.u8 d5,d4,d5        ; d5 = [ 0, a+b+c+d+z ]
    vuzp.32 q1,q2           ; d5 = d4 = [ 0 ], d3 = [ a+b+c+d+z, a+b+c+z ], d2 = [ a+b+z, a+z ]
    vsri.64 d1,d3,#32       ; d1 = [ 0, a+b+c+d+z ]
.endm

; predict_pixels_left_2()
;
; Register inputs: d0 = [ 0, z ]
;                  d1 = src
;                  d2 = [ 0, 0 ]
; Register outputs: d1 = result
; Register clobbers: None
.macro predict_pixels_left_2
    vzip.32 d1,d2           ; d2 = [ 0, b ], d1 = [ 0, a ]
    vadd.u8 d1,d1,d0        ; d2 = [ 0, b ], d1 = [ 0, a+z ]
    vadd.u8 d2,d1,d2        ; d2 = [ 0, a+b+z ], d1 = [ 0, a+z ]
    vuzp.32 d1,d2           ; d2 = [ 0 ], d1 = [ a+b+z, a+z ]
    vsri.32 d0,d1,#32       ; d0 = [ 0, a+b+z ]
.endm

; parng_predict_scanline_left_packed_32bpp(uint8x4 *dest,
;                                          uint8x4 *src,
;                                          uint8x4 *prev,
;                                          uint64_t length,
;                                          uint64_t stride)
parng_predict_scanline_left_packed_32bpp:
    prolog
    veor.u8 q0,q0
    veor.u8 q2,q2
    loop_start
    predict_pixels_left_4
    loop_end 16,16
    epilog

; parng_predict_scanline_left_strided_32bpp(uint8x4 *dest,
;                                           uint8x4 *src,
;                                           uint8x4 *prev,
;                                           uint64_t length,
;                                           uint64_t stride)
parng_predict_scanline_left_strided_32bpp:
    prolog
    veor.u8 d0,d0
    loop_start
    vld4.8 d1,[src]
    vadd.u8 d0,d0,d1
    vst4.8 d0,[dest]
    loop_end_stride 4
    epilog

; parng_predict_scanline_left_packed_24bpp(uint8x4 *dest,
;                                          uint8x3 *src,
;                                          uint8x4 *prev,
;                                          uint64_t length,
;                                          uint64_t stride)
parng_predict_scanline_left_packed_24bpp:
    prolog
    load_24bpp_to_32bpp_table_lookup_mask d3
    load_32bpp_opaque_alpha_mask d4
    veor.u0 q0,q0           ; d0 = [ 0 ], d1 = [ 0 ]
    loop_start
    vld8.16 {d1},[src]      ; d1 = src (24bpp)
    vtbl.8 d1,{d2},d1       ; d1 = [ b, a ]
    predict_pixels_left_2
    vorr.u8 d1,d3,d1        ; d1 = result with alpha == 0xff
    vst8.16 {d1},[dest]     ; write pixels
    loop_end 8,6
    epilog

; parng_predict_scanline_left_strided_24bpp(uint8x4 *dest,
;                                           uint8x3 *src,
;                                           uint8x4 *prev,
;                                           uint64_t length,
;                                           uint64_t stride)
parng_predict_scanline_left_strided_24bpp:
    // TODO(pcwalton)

.macro predict_pixels_average dest,src,prev
    vld4.8 {d1},[\prev]     ; d1 = prev (8-bit)
    vld4.8 {d2},[\src]      ; d2 = src (8-bit)
    vhadd.u8 d0,d0,d1       ; d0 = avg(a, b) (8-bit)
    vadd.u8 d0,d0,d2        ; d0 = src + avg(a, b)
    vst4.8 {d0},[\dest]     ; write output pixel
.endm

; Register inputs: d0 = a, d1 = 0, d2 = c, d3 = 0
.macro predict_pixels_paeth dest,src,prev
    vld4.8 {d1},[\prev]     ; d1 = b (8-bit)
    vzip.8 d1,d3            ; d1 = b (16-bit); d3 = 0

    vsub.s16 d4,d2,d1       ; d4 = c - b = ±pa
    vsub.s16 d5,d0,d2       ; d5 = a - c = ±pb
    vabd.s16 d6,d5,d4       ; d6 = |a - c - c + b| = |a + b - 2c| = pc
    vabs.s16 q2,q2          ; d4 = pa, d5 = pb
    vmin.s16 d7,d4,d5       ; d7 = min(pa, pb)
    vcgt.s16 d4,d4,d5       ; d4 = pa > pb = ¬(pa ≤ pb)
    vcgt.s16 d7,d7,d6       ; d7 = min(pa, pb) > pc = ¬(pa ≤ pc) ∧ ¬(pb ≤ pc)
    vbic.u16 d5,d0,d4       ; d5 = a if pa ≤ pb
    vand.u16 d4,d4,d1       ; d4 = b if ¬(pa ≤ pb)
    vorr.u16 d4,d4,d7       ; d4 = ¬(pa ≤ pc) ∧ ¬(pb ≤ pc) ? TRUE : ¬(pa ≤ pb) ? b : FALSE
    vorr.u16 d5,d5,d4       ; d5 = ¬(pa ≤ pc) ∧ ¬(pb ≤ pc) ? TRUE : pa ≤ pb ? a : b
    vand.u16 d4,d4,d2       ; d7 = ¬(pa ≤ pc) ∧ ¬(pb ≤ pc) ? c : ¬(pa ≤ pb) ? undef : FALSE
    vmax.s16 d4,d5,d4       ; d4 = ¬(pa ≤ pc) ∧ ¬(pb ≤ pc) ? c : (pa ≤ pb) ∧ (pa ≤ pc) ? a : b
    vld4.8 {d0},[\src]      ; d0 = original pixel (8-bit)
    vzip.8 d0,d3            ; d0 = original pixel (16-bit); d3 = 0
    vadd.u8 d0,d4           ; d0 = next a = output pixel
    vmov d4,d0              ; d4 = output pixel (16-bit)
    vuzp.8 d4,d3            ; d0 = output pixel (8-bit); d3 = 0
    vst4.8 {d3},[\dest]     ; write output pixel
    vmov d2,d1              ; c = b
.endm

