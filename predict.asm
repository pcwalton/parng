; parng/predict.asm

bits 64

global parng_predict_scanline_none_packed_32bpp
global parng_predict_scanline_none_strided_32bpp
global parng_predict_scanline_none_packed_24bpp
global parng_predict_scanline_none_strided_24bpp
global parng_predict_scanline_none_packed_8bpp
global parng_predict_scanline_left_packed_32bpp
global parng_predict_scanline_left_strided_32bpp
global parng_predict_scanline_left_packed_24bpp
global parng_predict_scanline_left_strided_24bpp
global parng_predict_scanline_left_packed_8bpp
global parng_predict_scanline_up_packed_32bpp
global parng_predict_scanline_up_strided_32bpp
global parng_predict_scanline_up_packed_24bpp
global parng_predict_scanline_up_strided_24bpp
global parng_predict_scanline_up_packed_8bpp
global parng_predict_scanline_average_strided_32bpp
global parng_predict_scanline_average_strided_24bpp
global parng_predict_scanline_paeth_strided_32bpp
global parng_predict_scanline_paeth_strided_24bpp

; Abstract over Windows and System V calling conventions.
%ifidn __OUTPUT_FORMAT__,win64
    %define dest rcx
    %define src rdx
    %define prev r8
    %define length r9
    %define stride r10
%else
    %define dest rdi
    %define src rsi
    %define prev rdx
    %define length rcx
    %define stride r8
%endif

section .text

; load_8bpp_to_32bpp_shuffle_mask(r128 dest)
;
; Register clobbers: rax
%macro load_8bpp_to_32bpp_shuffle_mask 1
    mov rax,0x0101010100000000
    movq %1,rax
    mov rax,0x0303030302020202
    pinsrq %1,rax,1                             ; dest = 8bpp → 32bpp shuffle mask
%endmacro

; load_24bpp_to_32bpp_shuffle_mask(r128 dest)
;
; Register clobbers: rax
%macro load_24bpp_to_32bpp_shuffle_mask 1
    mov rax,0x8005040380020100
    movq %1,rax
    mov rax,0x800b0a0980080706
    pinsrq %1,rax,1                             ; dest = 24bpp → 32bpp shuffle mask
%endmacro

; load_64bpp_to_32bpp_shuffle_mask(r128 dest)
;
; Register clobbers: rax
%macro load_64bpp_to_32bpp_shuffle_mask 1
    mov rax,0x8080808006040200  ; rax = 64bpp → 32bpp shuffle mask
    movq %1,rax                 ; dest = 64bpp → 32bpp shuffle mask
%endmacro

; load_64bpp_to_32bpp_opaque_alpha_shuffle_mask(r128 dest)
;
; Register clobbers: rax
%macro load_64bpp_to_32bpp_opaque_alpha_shuffle_mask 1
    mov rax,0x8080808080040200  ; rax = 64bpp → 32bpp opaque alpha shuffle mask
    movq %1,rax                 ; dest = 64bpp → 32bpp opaque alpha shuffle mask
%endmacro

; load_32bpp_opaque_alpha_mask(r128 dest)
;
; Register clobbers: rax
%macro load_32bpp_opaque_alpha_mask 1
    mov rax,0xff000000ff000000
    movq %1,rax
    movddup %1,%1               ; dest = [ ff000000 x 4 ]
%endmacro

; parng_predict_scanline_none_packed_32bpp(uint8x4 *dest,
;                                          uint8x4 *src,
;                                          uint8x4 *prev,
;                                          uint64_t length,
;                                          uint64_t stride)
;
; FIXME(pcwalton): We don't need two adds here, but I'm leaving them in in the chance that we can
; factor out the loop into a macro for safety.
parng_predict_scanline_none_packed_32bpp:
    xor rax,rax
.loop:
    movdqa xmm0,[src]
    movdqa [dest+rax],xmm0
    add rax,16
    add src,16
    cmp rax,length
    jb .loop
    ret

; parng_predict_scanline_none_strided_32bpp(uint8x4 *dest,
;                                           uint8x4 *src,
;                                           uint8x4 *prev,
;                                           uint64_t length,
;                                           uint64_t stride)
parng_predict_scanline_none_strided_32bpp:
    xor rax,rax
.loop:
    mov r10,[src]
    mov [dest+rax],r10
    add rax,stride
    add src,4
    cmp rax,length
    jb .loop
    ret

; parng_predict_scanline_none_packed_24bpp(uint8x4 *dest,
;                                          uint8x4 *src,
;                                          uint8x4 *prev,
;                                          uint64_t length,
;                                          uint64_t stride)
parng_predict_scanline_none_packed_24bpp:
    load_24bpp_to_32bpp_shuffle_mask xmm1
    xor rax,rax
.loop:
    movdqu xmm0,[src]
    pshufb xmm0,xmm1
    movdqa [dest+rax],xmm0
    add src,12
    add rax,16
    cmp rax,length
    jb .loop
    ret

; parng_predict_scanline_none_strided_24bpp(uint8x4 *dest,
;                                           uint8x4 *src,
;                                           uint8x4 *prev,
;                                           uint64_t length,
;                                           uint64_t stride)
parng_predict_scanline_none_strided_24bpp:
    xor rax,rax
.loop:
    mov r10d,[src]
    and r10d,0x00ffffff
    mov [dest+rax],r10d
    add src,4
    add rax,stride
    cmp rax,length
    jb .loop
    ret

; parng_predict_scanline_none_packed_8bpp(uint8x4 *dest,
;                                         uint8x4 *src,
;                                         uint8x4 *prev,
;                                         uint64_t length,
;                                         uint64_t stride)
parng_predict_scanline_none_packed_8bpp:
    load_8bpp_to_32bpp_shuffle_mask xmm1
    xor rax,rax
.loop:
    movd xmm0,[src]
    pshufb xmm0,xmm1
    movdqa [dest+rax],xmm0
    add src,4
    add rax,16
    cmp rax,length
    jb .loop
    ret

; predict_pixels_left(r/m128 dest, r/m128 src)
;
; Register inputs: xmm0 = [ 0, 0, 0, z ]
; Register outputs: xmm0 = [ 0, 0, 0, a+b+c+d+z ] (i.e. z for next batch of pixels)
; Register clobbers: xmm1
;
; https://github.com/kobalicek/simdtests/blob/master/depng/depng_sse2.cpp
%macro predict_pixels_left 2
    paddb xmm0,%2                               ; xmm0 = [ d,         c,       b,     a+z       ]
    vpslldq xmm1,xmm0,8                         ; xmm1 = [ b,         a,       0,     0         ]
    paddb xmm0,xmm1                             ; xmm0 = [ b+d,       a+c+z,   b,     a+z       ]
    vpslldq xmm1,xmm0,4                         ; xmm1 = [ a+c+z,     b,       a+e,   0         ]
    paddb xmm0,xmm1                             ; xmm0 = [ a+b+c+d+z, a+b+c+z, a+b+e, a+z       ]
    movdqa %1,xmm0                              ; write result
    vpsrldq xmm0,12                             ; xmm0 = [ 0,         0,       0,     a+b+c+d+z ]
%endmacro

; parng_predict_scanline_left_packed_32bpp(uint8x4 *dest,
;                                          uint8x4 *src,
;                                          uint8x4 *prev,
;                                          uint64_t length,
;                                          uint64_t stride)
;
; FIXME(pcwalton): We don't need two adds here, but I'm leaving them in in the chance that we can
; factor out the loop into a macro for safety.
parng_predict_scanline_left_packed_32bpp:
    xorps xmm0,xmm0
    xor rax,rax
.loop:
    predict_pixels_left [dest+rax],[src]    ; xmm0 = [ 0, 0, 0, a+b+c+d+z ]
    add rax,16
    add src,16
    cmp rax,length
    jb .loop
    ret

; parng_predict_scanline_left_strided_32bpp(uint8x4 *dest,
;                                           uint8x4 *src,
;                                           uint8x4 *prev,
;                                           uint64_t length,
;                                           uint64_t stride)
;
; FIXME(pcwalton): We don't need two adds here, but I'm leaving them in in the chance that we can
; factor out the loop into a macro for safety.
parng_predict_scanline_left_strided_32bpp:
    xorps xmm0,xmm0
    xor rax,rax
.loop:
    movd xmm1,[src]
    paddb xmm0,xmm1
    movd [dest+rax],xmm0
    add src,4
    add rax,stride
    cmp rax,length
    jb .loop
    ret

; parng_predict_scanline_left_packed_24bpp(uint8x4 *dest,
;                                          uint8x3 *src,
;                                          uint8x4 *prev,
;                                          uint64_t length,
;                                          uint64_t stride)
parng_predict_scanline_left_packed_24bpp:
    load_24bpp_to_32bpp_shuffle_mask xmm2       ; xmm2 = 24bpp → 32bpp shuffle mask
    load_32bpp_opaque_alpha_mask xmm3           ; xmm3 = opaque alpha mask
    xorps xmm0,xmm0                             ; xmm0 = a = 0
    xor rax,rax
.loop:
    movdqu xmm1,[src]                           ; xmm1 = src (24bpp)
    pshufb xmm1,xmm2                            ; xmm1 = [ d, c, b, a ]
    predict_pixels_left xmm1,xmm1               ; xmm1 = result; xmm0 = [ 0, 0, 0, a+b+c+d+z ]
    por xmm1,xmm3                               ; xmm1 = result with alpha == 0xff
    movdqa [dest+rax],xmm1
    add src,12
    add rax,16
    cmp rax,length
    jb .loop
    ret

; parng_predict_scanline_left_strided_24bpp(uint8x4 *dest,
;                                           uint8x3 *src,
;                                           uint8x4 *prev,
;                                           uint64_t length,
;                                           uint64_t stride)
parng_predict_scanline_left_strided_24bpp:
    load_24bpp_to_32bpp_shuffle_mask xmm2       ; xmm2 = 24bpp → 32bpp shuffle mask
    load_32bpp_opaque_alpha_mask xmm3           ; xmm3 = opaque alpha mask
    xorps xmm0,xmm0                             ; xmm0 = a = 0
    xor rax,rax
.loop:
    movd xmm1,[src]                             ; xmm1 = src (24bpp)
    paddb xmm0,xmm1
    por xmm0,xmm3                               ; xmm1 = result with alpha == 0xff
    movd [dest+rax],xmm0
    add src,3
    add rax,stride
    cmp rax,length
    jb .loop
    ret

; parng_predict_scanline_left_packed_8bpp(uint8x4 *dest,
;                                         uint8x3 *src,
;                                         uint8x4 *prev,
;                                         uint64_t length,
;                                         uint64_t stride)
parng_predict_scanline_left_packed_8bpp:
    load_8bpp_to_32bpp_shuffle_mask xmm2        ; xmm2 = 8bpp → 32bpp shuffle mask
    xorps xmm0,xmm0                             ; xmm0 = a = 0
    xor rax,rax
.loop:
    movd xmm1,[src]                             ; xmm1 = src (8bpp)
    pshufb xmm1,xmm2                            ; xmm1 = [ d, c, b, a ]
    predict_pixels_left xmm1,xmm1               ; xmm1 = result; xmm0 = [ 0, 0, 0, a+b+c+d+z ]
    movdqa [dest+rax],xmm1
    add src,4
    add rax,16
    cmp rax,length
    jb .loop
    ret

; parng_predict_scanline_up_packed_32bpp(uint8x4 *dest,
;                                        uint8x4 *src,
;                                        uint8x4 *prev,
;                                        uint64_t length,
;                                        uint64_t stride)
parng_predict_scanline_up_packed_32bpp:
    xor rax,rax
.loop:
    movdqa xmm0,[prev+rax]                      ; xmm0 = prev
    paddb xmm0,[src]                            ; xmm0 = prev + this
    movdqa [dest+rax],xmm0                      ; write result
    add src,16
    add rax,16
    cmp rax,length
    jb .loop
    ret

; parng_predict_scanline_up_strided_32bpp(uint8x4 *dest,
;                                         uint8x4 *src,
;                                         uint8x4 *prev,
;                                         uint64_t length,
;                                         uint64_t stride)
parng_predict_scanline_up_strided_32bpp:
    xor rax,rax
.loop:
    movd xmm0,[prev+rax]                        ; xmm0 = prev
    movd xmm1,[src]                             ; xmm1 = this
    paddb xmm0,xmm1                             ; xmm0 = prev + this
    movd [dest+rax],xmm0                        ; write result
    add src,4
    add rax,stride
    cmp rax,length
    jb .loop
    ret

; parng_predict_scanline_up_packed_24bpp(uint8x4 *dest,
;                                        uint8x3 *src,
;                                        uint8x4 *prev,
;                                        uint64_t length,
;                                        uint64_t stride)
;
; There is no need to make the alpha opaque here as long as the previous scanline had opaque alpha.
parng_predict_scanline_up_packed_24bpp:
    load_24bpp_to_32bpp_shuffle_mask xmm2       ; xmm2 = 24bpp → 32bpp shuffle mask
    xor rax,rax
.loop:
    movdqa xmm0,[prev+rax]                      ; xmm0 = prev
    movdqu xmm1,[src]                           ; xmm1 = src (24bpp)
    pshufb xmm1,xmm2                            ; xmm1 = src (32bpp)
    paddb xmm0,xmm1                             ; xmm0 = prev + this
    movdqa [dest+rax],xmm0                      ; write result
    add src,12
    add rax,16
    cmp rax,length
    jb .loop
    ret

; parng_predict_scanline_up_strided_24bpp(uint8x4 *dest,
;                                         uint8x3 *src,
;                                         uint8x4 *prev,
;                                         uint64_t length,
;                                         uint64_t stride)
;
; There is no need to make the alpha opaque here as long as the previous scanline had opaque alpha.
parng_predict_scanline_up_strided_24bpp:
    load_24bpp_to_32bpp_shuffle_mask xmm2       ; xmm2 = 24bpp → 32bpp shuffle mask
    xor rax,rax
.loop:
    movd xmm0,[prev+rax]                        ; xmm0 = prev
    movd xmm1,[src]                             ; xmm1 = src
    paddb xmm0,xmm1                             ; xmm1 = prev + src (24bpp)
    pshufb xmm0,xmm2                            ; xmm1 = prev + src (32bpp)
    movd [dest+rax],xmm0                        ; write result
    add src,3
    add rax,stride
    cmp rax,length
    jb .loop
    ret

; parng_predict_scanline_up_packed_8bpp(uint8x4 *dest,
;                                       uint8 *src,
;                                       uint8x4 *prev,
;                                       uint64_t length,
;                                       uint64_t stride)
parng_predict_scanline_up_packed_8bpp:
    load_8bpp_to_32bpp_shuffle_mask xmm2        ; xmm2 = 24bpp → 32bpp shuffle mask
    xor rax,rax
.loop:
    movdqa xmm0,[prev+rax]                      ; xmm0 = prev
    movdqu xmm1,[src]                           ; xmm1 = src (24bpp)
    pshufb xmm1,xmm2                            ; xmm1 = src (32bpp)
    paddb xmm0,xmm1                             ; xmm0 = prev + this
    movdqa [dest+rax],xmm0                      ; write result
    add src,12
    add rax,16
    cmp rax,length
    jb .loop
    ret

; predict_pixels_average(r/m32 dest, xmm/m32 src, xmm/m64 prev)
;
; Register inputs: xmm0 = a (16-bit), xmm3 = 16 → 8 shuffle mask
; Register outputs: xmm0 = dest (16-bit) (i.e. a for next pixel)
; Register clobbers: xmm1, xmm2
;
; This is sequential across pixels since there's really no way to eliminate the data dependency
; that I can see. STOKE couldn't find a way either.
;
; FIXME(pcwalton): This could be shorter with an unrolled loop.
%macro predict_pixels_average 3
    pmovzxbw xmm1,%3                            ; xmm1 = b (16-bit)
    movd xmm2,%2                                ; xmm2 = src (8-bit)
    paddw xmm0,xmm1                             ; xmm0 = a + b (16-bit)
    psrlw xmm0,1                                ; xmm0 = avg(a, b) (16-bit)
    pshufb xmm0,xmm3                            ; xmm0 = avg(a, b) (8-bit)
    paddb xmm0,xmm2                             ; xmm0 = src + avg(a, b)
    movd %1,xmm0                                ; write this
    pmovzxbw xmm0,xmm0                          ; xmm0 = a (16-bit)
%endmacro

; parng_predict_scanline_average_strided_32bpp(uint8x4 *dest,
;                                              uint8x4 *src,
;                                              uint8x4 *prev,
;                                              uint64_t length,
;                                              uint64_t stride)
parng_predict_scanline_average_strided_32bpp:
    load_64bpp_to_32bpp_shuffle_mask xmm3       ; rax = 64bpp → 32bpp shuffle mask
    xorps xmm0,xmm0                             ; xmm0 = a = 0
    xor rax,rax
.loop:
    predict_pixels_average [dest+rax],[src],[prev+rax]  ; xmm0 = a (16-bit)
    add src,4
    add rax,stride
    cmp rax,length
    jb .loop
    ret

; parng_predict_scanline_average_strided_24bpp(uint8x4 *dest,
;                                              uint8x3 *src,
;                                              uint8x4 *prev,
;                                              uint64_t length,
;                                              uint64_t stride)
parng_predict_scanline_average_strided_24bpp:
    mov rax,0x8080808080040200                  ; rax = 64bpp → 32bpp shuffle mask (no alpha!)
    movq xmm3,rax                               ; xmm3 = 64bpp → 32bpp shuffle mask (no alpha!)
    xorps xmm0,xmm0                             ; xmm0 = a = 0
    xor rax,rax
.loop:
    predict_pixels_average r9d,[src],[prev+rax] ; r9 = a (16-bit)
    or r9d,0xff000000
    mov [dest+rax],r9d
    add src,3
    add rax,stride
    cmp rax,length
    jb .loop
    ret

; predict_pixels_paeth(r/m32 dest, xmm/m64 src, xmm/m64 prev)
;
; Register inputs: xmm0 = a (16-bit), xmm2 = c (16-bit), xmm10 = 64bpp → 32bpp shuffle mask
; Register outputs: xmm0 = next a (16-bit), xmm2 = next c (16-bit)
; Register clobbers: xmm1, xmm3, xmm4, xmm5, xmm6, xmm7, xmm8
;
; This is based on the spec'd Paeth filter, optimized using the STOKE superoptimizer and manually
; cleaned up.
;
; See the public domain code here for a completely different algorithm:
; https://github.com/kobalicek/simdtests/blob/master/depng/depng_sse2.cpp
;
; That code is shorter in instruction count but depends on a division by 3, which requires the
; high-latency `pmulhw` instruction (5 cycles on Haswell). It's worth possibly switching to if the
; latency on that instruction goes down.
;
; The main trick here is to use `pmaxsw` on a combination of values and Boolean results, keeping in
; mind that true in SSE is represented as -1 and all of our other values at that point are
; nonnegative.
%macro predict_pixels_paeth 3
    pmovzxbw xmm1,%3            ; xmm1 = b (16-bit)

    vpsubw xmm4,xmm2,xmm1       ; xmm4 = c - b = ±pa
    vpsubw xmm5,xmm0,xmm2       ; xmm5 = a - c = ±pb
    vpsubw xmm6,xmm5,xmm4       ; xmm6 = a - c - c + b = a + b - 2c = ±pc
    pabsw xmm4,xmm4             ; xmm4 = pa
    pabsw xmm5,xmm5             ; xmm5 = pb
    pabsw xmm6,xmm6             ; xmm6 = pc
    vpminsw xmm7,xmm4,xmm5      ; xmm7 = min(pa, pb)
    pcmpgtw xmm4,xmm5           ; xmm4 = pa > pb = ¬(pa ≤ pb)
    pcmpgtw xmm7,xmm6           ; xmm7 = min(pa, pb) > pc = ¬(pa ≤ pc) ∧ ¬(pb ≤ pc)
    vpandn xmm8,xmm4,xmm0       ; xmm8 = a if pa ≤ pb
    pand xmm4,xmm1              ; xmm4 = b if ¬(pa ≤ pb)
    por xmm4,xmm7               ; xmm7 = ¬(pa ≤ pc) ∧ ¬(pb ≤ pc) ? TRUE : ¬(pa ≤ pb) ? b : FALSE
    por xmm8,xmm4               ; xmm8 = ¬(pa ≤ pc) ∧ ¬(pb ≤ pc) ? TRUE : pa ≤ pb ? a : b
    pand xmm4,xmm2              ; xmm7 = ¬(pa ≤ pc) ∧ ¬(pb ≤ pc) ? c : ¬(pa ≤ pb) ? undef : FALSE
    pmaxsw xmm8,xmm4            ; xmm8 = ¬(pa≤pc) ∧ ¬(pb≤pc) ? c : (pa≤pb) ∧ (pa≤pc) ? a : b

    pmovzxbw xmm0,%2            ; xmm0 = original pixel (16 bit)
    paddb xmm0,xmm8             ; xmm0 = next a = output pixel
    vpshufb xmm3,xmm0,xmm10     ; xmm3 = output pixel (8-bit)
    movd %1,xmm3                ; write output pixel
    movdqa xmm2,xmm1            ; c = b
%endmacro

; parng_predict_scanline_paeth_strided_32bpp(uint8x4 *dest,
;                                            uint8x4 *src,
;                                            uint8x4 *prev,
;                                            uint64_t length,
;                                            uint64_t stride)
parng_predict_scanline_paeth_strided_32bpp:
    load_64bpp_to_32bpp_shuffle_mask xmm10  ; xmm10 = 64bpp → 32bpp shuffle mask
    xorps xmm0,xmm0             ; xmm0 = a = 0
    xorps xmm2,xmm2             ; xmm2 = c = 0
    xor rax,rax
.loop:
    predict_pixels_paeth [dest+rax],[src],[prev+rax]
    add src,4
    add rax,stride
    cmp rax,length
    jb .loop
    ret

; parng_predict_scanline_paeth_strided_24bpp(uint8x4 *dest,
;                                            uint8x3 *src,
;                                            uint8x4 *prev,
;                                            uint64_t length,
;                                            uint64_t stride)
parng_predict_scanline_paeth_strided_24bpp:
    load_64bpp_to_32bpp_opaque_alpha_shuffle_mask xmm10 ; xmm10 = 64bpp → 32bpp shuffle mask
    xorps xmm0,xmm0             ; xmm0 = a = 0
    xorps xmm2,xmm2             ; xmm2 = c = 0
    xor rax,rax
.loop:
    predict_pixels_paeth r9d,[src],[prev+rax]
    or r9d,0xff000000
    mov [dest+rax],r9d
    add src,3
    add rax,stride
    cmp rax,length
    jb .loop
    ret

