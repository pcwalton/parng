; parng/prediction-arm-neon.asm
;
; Copyright (c) 2016 Mozilla Foundation

parng_predict_scanline_none_packed_32bpp:
    prolog
    loop_start
    vld4.8 {d0-d3},[src]
    vst4.8 {d0-d3},[dest]
    loop_end 16,16
    epilog

.macro predict_pixels_left
    ; TODO(pcwalton)
.endm

.macro predict_pixels_average
    vld4.8 {d1},[prev]  ; d1 = prev (8-bit)
    vld4.8 {d2},[src]   ; d2 = src (8-bit)
    vhadd.u8 d0,d0,d1   ; d0 = avg(a, b) (8-bit)
    vadd.u8 d0,d0,d2    ; d0 = src + avg(a, b)
    vst4.8 {d0},[dest]  ; write output pixel
.endm

.macro predict_pixels_paeth
    vld4.8 {d1},[prev]  ; d1 = b (8-bit)
    veor.8 d3,d3,d3     ; d3 = 0
    vzip.8 d1,d3        ; d1 = b (16-bit); d3 = junk

    vsub.s16 d4,d2,d1   ; d4 = c - b = ±pa
    vsub.s16 d5,d0,d2   ; d5 = a - c = ±pb
    vsub.s16 d6,d5,d4   ; d6 = a - c - c + b = a + b - 2c = ±pc
    vabs.s16 q2,q2      ; d4 = pa, d5 = pb
    vabs.s16 d6,d6      ; d6 = pc
    vmin.s16 d7,d4,d5   ; d7 = min(pa, pb)
    vcgt.s16 d4,d4,d5   ; d4 = pa > pb = ¬(pa ≤ pb)
    vcgt.s16 d7,d7,d6   ; d7 = min(pa, pb) > pc = ¬(pa ≤ pc) ∧ ¬(pb ≤ pc)
    vbic.16 d5,d0,d4    ; d5 = a if pa ≤ pb
    vand.16 d4,d4,d1    ; d4 = b if ¬(pa ≤ pb)
    vorr.16 d4,d4,d7    ; d4 = ¬(pa ≤ pc) ∧ ¬(pb ≤ pc) ? TRUE : ¬(pa ≤ pb) ? b : FALSE
    vorr.16 d5,d5,d4    ; d5 = ¬(pa ≤ pc) ∧ ¬(pb ≤ pc) ? TRUE : pa ≤ pb ? a : b
    vand.16 d4,d4,d2    ; d7 = ¬(pa ≤ pc) ∧ ¬(pb ≤ pc) ? c : ¬(pa ≤ pb) ? undef : FALSE
    vmax.s16 d4,d5,d4   ; d4 = ¬(pa ≤ pc) ∧ ¬(pb ≤ pc) ? c : (pa ≤ pb) ∧ (pa ≤ pc) ? a : b
    veor.8 d3,d3,d3     ; d3 = 0
    vld4.8 {d0},src     ; d0 = original pixel (8-bit)
    vzip.8 d0,d3        ; d0 = original pixel (16-bit); d3 = junk
    vadd.u8 d0,d4       ; d0 = next a = output pixel
    vmov d3,d0          ; d3 = output pixel (16-bit)
    vuzp.8 d3,d3        ; d0 = output pixel (8-bit)
    vst4.8 {d3},[dest]  ; write output pixel
    vmov d2,d1          ; c = b
.endm

