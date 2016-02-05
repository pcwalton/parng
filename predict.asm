; parng/predict.asm

bits 64

global parng_predict_scanline_left
global parng_predict_scanline_up
global parng_predict_scanline_average
global parng_predict_scanline_paeth

; parng_predict_scanline_left(uint8x4 *this, uint8x4 *prev, uint64_t width)
;
; https://github.com/kobalicek/simdtests/blob/master/depng/depng_sse2.cpp
parng_predict_scanline_left:
    xorps xmm0,xmm0
    xor rax,rax
.loop:
    paddb xmm0,[rdi+rax*4]                      ; xmm0 = [ d,         c,       b,     a+z       ]
    vpslldq xmm1,xmm0,8                         ; xmm1 = [ b,         a,       0,     0         ]
    paddb xmm0,xmm1                             ; xmm0 = [ b+d,       a+c+z,   b,     a+z       ]
    vpslldq xmm1,xmm0,4                         ; xmm1 = [ a+c+z,     b,       a+e,   0         ]
    paddb xmm0,xmm1                             ; xmm0 = [ a+b+c+d+z, a+b+c+z, a+b+e, a+z       ]
    movdqu [rdi+rax*4],xmm0                     ; write result
    vpsrldq xmm0,12                             ; xmm0 = [ 0,         0,       0,     a+b+c+d+z ]
    add rax,4
    cmp rax,rdx
    jb .loop
    ret

; parng_predict_scanline_up(uint8x4 *this, uint8x4 *prev, uint64_t width)
parng_predict_scanline_up:
    xor rax,rax
.loop:
    movdqu xmm0,[rdi+rax*4]                     ; xmm0 = this
    paddb xmm0,[rsi+rax*4]
    movdqu [rdi+rdx*4],xmm0                     ; write result
    add rax,4
    cmp rax,rdx
    jb .loop
    ret

; parng_predict_scanline_average(uint8x4 *this, uint8x4 *prev, uint64_t width)
parng_predict_scanline_average:
    xorps xmm0,xmm0                             ; xmm0 = a
    xor rax,rax
.loop:
    vpavgb xmm1,xmm0,[rsi+rax*4]                ; xmm1 = avg(a, b)
    vpaddb xmm0,xmm1,[rdi+rax*4]                ; xmm0 = this + avg(a, b)
    movd [rdi+rax*4],xmm0                       ; write this
    inc rax
    cmp rax,rdx
    jb .loop
    ret

; parng_predict_scanline_paeth(uint8x4 *this, uint8x4 *prev, uint64_t width)
;
; https://github.com/kobalicek/simdtests/blob/master/depng/depng_sse2.cpp
parng_predict_scanline_paeth:
    mov rax,0x5580558055805580
    movq xmm0,rax
    xorps xmm1,xmm1                             ; xmm1 = a = 0
    xorps xmm3,xmm3                             ; xmm3 = c = 0
    xor rax,rax
.loop:
    pmovzxbw xmm2,[rsi+rax*4]                   ; xmm2 = b
    vpminsw xmm4,xmm1,xmm2                      ; xmm4 = min(a, b)
    vpmaxsw xmm5,xmm1,xmm2                      ; xmm5 = max(a, b)
    vpsubw xmm6,xmm5,xmm4                       ; xmm6 = |a - b|
    pmulhw xmm6,xmm0                            ; xmm6 = |a - b|/3
    psubw xmm4,xmm3                             ; xmm4 = min(a, b) - c
    psubw xmm5,xmm3                             ; xmm5 = max(a, b) - c
    vpcmpgtw xmm7,xmm6,xmm4                     ; xmm7 = |a - b|/3 > min(a, b) - c
    vpandn xmm7,xmm5,xmm7                       ; xmm7 = max(a, b) - c if we should choose it
    pcmpgtw xmm5,xmm6                           ; xmm5 = max(a, b) - c > |a - b|/3
    pand xmm5,xmm4                              ; xmm5 = min(a, b) - c if we should choose it
    paddw xmm3,xmm7                             ; xmm3 = max(a, b) if we should choose it; else c
    paddw xmm3,xmm5                             ; xmm3 = result
    pmovzxbw xmm1,[rdi+rax*4]                   ; xmm1 = a = result
    paddw xmm1,xmm3                             ; xmm1 = output pixel
    vpackuswb xmm4,xmm1,xmm1                    ; xmm1 = output pixel (8-bit)
    movd [rsi+rax*4],xmm4                       ; write output pixel
    movdqa xmm3,xmm2                            ; c = b
    add rax,4
    cmp rax,rdx
    jb .loop
    ret

