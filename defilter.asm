; parng/defilter.asm

; defilter_left_scanline(uint8x4 *this, uint8x4 *prev, uint64_t width)
defilter_left_scanline:
    xor rax,rax
.loop:
    

; defilter_up_scanline(uint8x4 *this, uint8x4 *prev, uint64_t width)
defilter_up_scanline:
    xor rax,rax
.loop:
    movdqu xmm0,[rdi+rax*4]                     ; xmm0 = this
    paddb xmm0,[rsi+rax*4]                      ; xmm1 = prev
    movdqu [rdi+rdx*4],xmm0                     ; write result
    add rax,4
    cmp rax,rdx
    jb .loop
    ret

; defilter_paeth_scanline(uint8x4 *this, uint8x4 *prev, uint64_t width)
defilter_paeth_scanline:
    xor rax,rax                                 ; rax = counter
    xorps xmm0,xmm0                             ; xmm0 = a (16-bit) = 0
    xorps xmm2,xmm2
    pinsrd xmm2,[rsi+rax*4]                     ; xmm2 = c (8-bit)
    pmovzxbw xmm2,xmm2                          ; xmm2 = c (16-bit)
    pcmpeqb xmm15,xmm15                         ; xmm15 = fffffffff...

.loop:
    pmovzxbw xmm1,[rsi+rax*4]                   ; xmm1 = b (16-bit)
    pinsrd xmm2,ecx,3                           ; xmm2 = c
    vpsubb xmm3,xmm0,xmm2                       ; xmm3 = ±pa
    pabsb xmm3,xmm3                             ; xmm3 = pa

    ; defilter_paeth_pixel(result_register)
    ; Expects:
    ;   xmm0 = a (16-bit)
    ;   xmm4 = c (16-bit)
    ;   xmm7 = b (16-bit)
    ;   xmm8 = pa (16-bit)
    ;   xmm15 = all 1s
    ; Clobbers: xmm3, xmm5, xmm6, xmm10, xmm11, xmm13, xmm14.
    ; Returns 16-bit result in `result_register`.
    ; Operation completes in 17 cycles.
%macro defilter_paeth_pixel 1
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

    vpshufb xmm8,xmm3,paeth_shuffle_mask_pa_01  ; xmm8 = pa (16-bit)
    vpshufb xmm4,xmm2,paeth_shuffle_mask_bc_01  ; xmm4 = c
    vpshufb xmm7,xmm1,paeth_shuffle_mask_bc_01  ; xmm7 = b
    defilter_paeth_pixel xmm12                  ; xmm12 = pixel 0

    pextrd edx,xmm7,0
    mov [dest],edx
                                                ;       = pc <= pa && (!(pc > pb) || !(pc > pa))
                                                ;       = pc <= pa && (pc <= pb || pc <= pa)
                                                ;       = pc <= pa && pc <= pb
                                                ;       = pa >= pb && pb >= pc
    ; !(pb > pa && !(pc > pb && pc > pa))
    ; !(pb > pa) || (pc > pb && pc > pa)
    ; pb <= pa || (pc > pb && pc > pa)
    ; pa >= pb || (pb < pc && pa < pc)

    vpminsw xmm9,xmm8,xmm11                     ; xmm9 = min(pa, pb)
    pminsw xmm9,xmm6                            ; xmm9 = min(pa, pb, pc)
    vpcmpeqw xmm10,xmm9,xmm8                    ; xmm10 = pa <= pb && pa <= pc
    vpcmpeqw xmm12,xmm9,xmm11                   ; xmm12 = pb <= pa && pb <= pc
    vpcmpeqw xmm13,xmm9,xmm6                    ; xmm13 = pc <= pa && pc <= pb
    pandn xmm12,xmm10                           ; xmm12 = pb <= pc && pa > pb
    pandn xmm13,xmm10
    pandn xmm13,xmm12                           ; xmm13 = pc is lowest
    pand xmm13,xmm7                             ; xmm13 = b if we should choose it
    pand xmm14,xmm4                             ; xmm14 = c if we should choose it
    por xmm13,xmm14                             ; xmm13 = b or c if we should choose one
    pshufb xmm13,xmm13,paeth_shuffle_mask_16_to_8
    pshufb xmm12,xmm12,paeth_shuffle_mask_16_to_8
    pand xmm12,xmm5
    por xmm12,xmm13
    pextrd edx,xmm12,0
    mov [dest],edx

paeth_one:
    .do 00010001_00010001_00010001_00010001h
paeth_shuffle_mask_bc_23:
    .do 800f800e_800d800c_800b800a_80098008h

; 18 cycles per byte; 72 cycles per pixel
naive_defilter_paeth_scanline:
    movzx eax,byte ptr [a]
    movzx ebx,byte ptr [b]
    movzx ecx,byte ptr [c]
    mov dx,ax
    add dx,bx
    sub dx,cx                                   ; dx = p
    mov si,dx
    sub si,ax
    ; abs si                                      ; si = pa
    mov di,dx
    sub di,bx
    ; abs di                                      ; di = pb
    sub dx,cx
    ; abs dx                                      ; dx = pc
    cmp si,di
    jge .write_it
    cmp di,dx
    jge .pick_b
    mov ax,cx
    jmp .write_it
.pick_b:
    mov ax,bx
.write_it:
    mov byte ptr [dest],al

