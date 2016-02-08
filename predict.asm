; parng/predict.asm

bits 64

global parng_predict_scanline_none
global parng_predict_scanline_left
global parng_predict_scanline_up
global parng_predict_scanline_average
global parng_predict_scanline_paeth

%ifidn __OUTPUT_FORMAT__,win64
    %define this_line rcx
    %define prev_line rdx
    %define width r8
%else
    %define this_line rdi
    %define prev_line rsi
    %define width rdx
%endif

section .text

; parng_predict_scanline_left(uint8x4 *this, uint8x4 *prev, uint64_t width)
;
; https://github.com/kobalicek/simdtests/blob/master/depng/depng_sse2.cpp
parng_predict_scanline_left:
    xorps xmm0,xmm0
    xor rax,rax
.loop:
    paddb xmm0,[this_line+rax*4]                ; xmm0 = [ d,         c,       b,     a+z       ]
    vpslldq xmm1,xmm0,8                         ; xmm1 = [ b,         a,       0,     0         ]
    paddb xmm0,xmm1                             ; xmm0 = [ b+d,       a+c+z,   b,     a+z       ]
    vpslldq xmm1,xmm0,4                         ; xmm1 = [ a+c+z,     b,       a+e,   0         ]
    paddb xmm0,xmm1                             ; xmm0 = [ a+b+c+d+z, a+b+c+z, a+b+e, a+z       ]
    movdqu [this_line+rax*4],xmm0               ; write result
    vpsrldq xmm0,12                             ; xmm0 = [ 0,         0,       0,     a+b+c+d+z ]
    add rax,4
    cmp rax,width
    jb .loop
    ret

; parng_predict_scanline_up(uint8x4 *this, uint8x4 *prev, uint64_t width)
parng_predict_scanline_up:
    xor rax,rax
.loop:
    movdqu xmm0,[prev_line+rax*4]               ; xmm0 = prev
    paddb xmm0,[this_line+rax*4]                ; xmm0 = prev + this
    movdqu [this_line+rax*4],xmm0               ; write result
    add rax,4
    cmp rax,width
    jb .loop
    ret

; parng_predict_scanline_average(uint8x4 *this, uint8x4 *prev, uint64_t width)
;
; This is sequential across pixels since there's really no way to eliminate the data dependency
; that I can see. STOKE couldn't find a way either.
;
; FIXME(pcwalton): This is really inefficient. Optimize this.
parng_predict_scanline_average:
    xorps xmm0,xmm0                             ; xmm0 = a
    xor rax,rax
.loop:
    movd xmm1,[prev_line+rax*4]                 ; xmm1 = b
    pavgb xmm0,xmm1                             ; xmm0 = avg(a, b)
    movd xmm1,[this_line+rax*4]                 ; xmm1 = a
    paddb xmm0,xmm1                             ; xmm0 = this + avg(a, b)
    movd [this_line+rax*4],xmm0                 ; write this
    inc rax
    cmp rax,width
    jb .loop
    ret

; parng_predict_scanline_paeth(uint8x4 *this, uint8x4 *prev, uint64_t width)
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
parng_predict_scanline_paeth:
    xorps xmm0,xmm0             ; xmm0 = a = 0
    xorps xmm2,xmm2             ; xmm2 = c = 0
    xor rax,rax
.loop:
    pmovzxbw xmm1,[prev_line+rax*4]   ; xmm1 = b
    vpsubw xmm4,xmm2,xmm1       ; xmm4 = c - b = ±pa
    vpsubw xmm5,xmm0,xmm2       ; xmm5 = a - c = ±pb
    vpsubw xmm6,xmm5,xmm4       ; xmm6 = a - c - c + b = a + b - 2c = ±pc
    pabsw xmm4,xmm4             ; xmm4 = pa
    pabsw xmm5,xmm5             ; xmm5 = pb
    pabsw xmm6,xmm6             ; xmm6 = pc
    vpminuw xmm7,xmm4,xmm5      ; xmm7 = min(pa, pb)
    pcmpgtw xmm4,xmm5           ; xmm4 = pa > pb = ¬(pa ≤ pb)
    vpandn xmm8,xmm4,xmm0       ; xmm8 = a if pa ≤ pb
    pcmpgtw xmm7,xmm6           ; xmm7 = min(pa, pb) > pc = ¬(pa ≤ pc) ∧ ¬(pb ≤ pc)
    pand xmm4,xmm1              ; xmm4 = b if ¬(pa ≤ pb)
    por xmm4,xmm7               ; xmm7 = ¬(pa ≤ pc) ∧ ¬(pb ≤ pc) ? TRUE : ¬(pa ≤ pb) ? b : FALSE
    por xmm8,xmm4               ; xmm8 = ¬(pa ≤ pc) ∧ ¬(pb ≤ pc) ? TRUE : pa ≤ pb ? a : b
    pand xmm7,xmm2              ; xmm7 = ¬(pa ≤ pc) ∧ ¬(pb ≤ pc) ? c : ¬(pa ≤ pb) ? undef : FALSE
    pmaxsw xmm8,xmm7            ; xmm8 = ¬(pa≤pc) ∧ ¬(pb≤pc) ? c : (pa≤pb) ∧ (pa≤pc) ? a : b
    pmovzxbw xmm0,[this_line+rax*4]
    paddw xmm0,xmm8             ; xmm0 = next a = output pixel
    vpackuswb xmm3,xmm0,xmm0    ; xmm3 = output pixel (8-bit)
    movd [this_line+rax*4],xmm3 ; write output pixel
    movdqa xmm2,xmm1            ; c = b
    inc rax
    cmp rax,width
    jb .loop
    ret

