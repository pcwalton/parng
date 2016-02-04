; parng/predict.asm

bits 64

global parng_predict_scanline_left
global parng_predict_scanline_up
global parng_predict_scanline_average
global parng_predict_scanline_paeth

; parng_predict_scanline_left(uint8x4 *this, uint8x4 *prev, uint64_t width)
parng_predict_scanline_left:
    xorps xmm0,xmm0
    xor rax,rax
.loop:
    paddb xmm0,[rdi+rax*4]
    movd [rdi+rax*4],xmm0
    inc rax
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
parng_predict_scanline_paeth:
    xor rax,rax                                 ; rax = counter
    xorps xmm0,xmm0                             ; xmm0 = a (16-bit) = 0
    movd xmm2,[rsi+rax*4]                       ; xmm2 = c (8-bit)
    pmovzxbw xmm2,xmm2                          ; xmm2 = c (16-bit)
    pcmpeqb xmm15,xmm15                         ; xmm15 = fffffffff...

.loop:
    pmovzxbw xmm1,[rsi+rax*4]                   ; xmm1 = b (16-bit)
    pinsrd xmm2,ecx,3                           ; xmm2 = c
    vpsubb xmm3,xmm0,xmm2                       ; xmm3 = ±pa
    pabsb xmm3,xmm3                             ; xmm3 = pa

    ; predict_paeth_pixel(result_register)
    ; Expects:
    ;   xmm0 = a (16-bit)
    ;   xmm4 = c (16-bit)
    ;   xmm7 = b (16-bit)
    ;   xmm8 = pa (16-bit)
    ;   xmm15 = all 1s
    ; Clobbers: xmm3, xmm5, xmm6, xmm10, xmm11, xmm13, xmm14.
    ; Returns 16-bit result in `result_register`.
    ; Operation completes in 17 cycles.
%macro predict_paeth_pixel 1
    vpsubw xmm5,xmm0,xmm4                       ; xmm11 = ±pb = a - c
    pabsw xmm11,xmm11                           ; xmm11 = pb
    vpsubw xmm6,xmm5,xmm4                       ; xmm6 = a - 2*c
    paddw xmm6,xmm7                             ; xmm6 = ±pc = a + b - 2*c
    pabsw xmm6,xmm6                             ; xmm6 = pc
    vpcmpgtw xmm9,xmm6,xmm11                    ; xmm9 = pc > pb
    vpcmpgtw xmm10,xmm11,xmm8                   ; xmm10 = pb > pa
    vpcmpgtw xmm11,xmm6,xmm8                    ; xmm11 = pc > pa
    vpand xmm13,xmm9,xmm11                      ; xmm13 = pc > pb && pc > pa
    vpandn xmm14,xmm10,xmm13                    ; xmm14 = pb > pa && !(pc > pb && pc > pa)
    vpandn xmm5,xmm15,xmm13                     ; xmm5 = !(pc > pa)
    pandn %1,xmm14                              ; xmm7 = !(pc > pa) && !(pc > pb && pc > pa)
                                                ; FIXME(pcwalton): Is this right?
    pand %1,xmm0                                ; xmm5 = a if we should use it
    pand xmm14,%1                               ; xmm14 = b if we should use it
    pand xmm13,xmm4                             ; xmm13 = c if we should use it
    por %1,xmm14                                ; xmm7 = a | b
    por %1,xmm13                                ; xmm3 = a | b | c
%endmacro

    ; vpshufb xmm8,xmm3,paeth_shuffle_mask_pa_01  ; xmm8 = pa (16-bit)
    ; vpshufb xmm4,xmm2,paeth_shuffle_mask_bc_01  ; xmm4 = c
    ; vpshufb xmm7,xmm1,paeth_shuffle_mask_bc_01  ; xmm7 = b
    predict_paeth_pixel xmm12                  ; xmm12 = pixel 0

    movd [rdi+rax*4],xmm7
    add rax,4
    cmp rax,rdx
    jb .loop
    ret

