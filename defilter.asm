; parng/defilter.asm

defilter_paeth_scanline:
    ; eax = last word of prev.
    ; edx = last word of this.
    movdqu xmm0,[prev]                          ; xmm0 = b (8-bit)
    movdqu xmm1,[this]                          ; xmm1 = this
    pshufd xmm2,xmm0,03030201b                  ; xmm2 = c[0..2]
    pinsrd xmm2,eax,3                           ; xmm2 = c
    vpsubb xmm3,xmm0,xmm2                       ; xmm3 = ±pa
    pabsb xmm3,xmm3                             ; xmm3 = pa

    ; 26 cycles per pixel; 2.77x improvement
    vpshufb xmm4,xmm2,paeth_shuffle_mask_bc_23  ; xmm4 = c
    vpshufb xmm7,xmm1,paeth_shuffle_mask_bc_23  ; xmm7 = b
    pinsrd xmm5,edx,3                           ; xmm5 = a
    vpsubw xmm11,xmm5,xmm4                      ; xmm11 = ±pb = a - c
    pabsw xmm11,xmm5                            ; xmm11 = pb
    vpsubw xmm6,xmm11,xmm4                      ; xmm6 = a - 2*c
    paddw xmm6,xmm1                             ; xmm6 = ±pc = a + b - 2*c
    pabsw xmm6,xmm6                             ; xmm6 = pc
    vpshufb xmm8,xmm3,paeth_shuffle_mask_pa_3   ; xmm8 = pa (16-bit)
    vpminsw xmm9,xmm11,xmm6                     ; xmm9 = min(pb, pc)
    paddw xmm9,paeth_one                        ; xmm9 = min(pb, pc) + 1
    pcmpgtw xmm9,xmm8                           ; xmm9 = pa <= pb && pa <= pc
    pshufb xmm9,xmm9,paeth_shuffle_mask_16_to_8 ; xmm9 = pa <= pb && pa <= pc (8-bit)
    vpand xmm12,xmm9,xmm5                       ; xmm12 = a if pa <= pb && pa <= pc
    vpaddw xmm10,xmm6,paeth_one                 ; xmm10 = pc + 1
    pcmpgtw xmm10,xmm11                         ; xmm10 = pb <= pc
    vpcmpgtw xmm11,xmm6,xmm11                   ; xmm11 = pc > pb = !(pc <= pb)
    vpandn xmm14,xmm10,xmm9                     ; xmm14 = pb <= pc && !(pa <= pb && pa <= pc)
    pandn xmm11,xmm9                            ; xmm11 = !(pb <= pc) && !(pa <= pb && pa <= pc)
    pand xmm14,xmm5                             ; xmm14 = b if pb<=pc && !(pa<=pb && pa<=pc)
    pand xmm15,xmm4                             ; xmm15 = c if !(pb<=pc) && !(pa<=pb && pa<=pc)
    por xmm14,xmm15                             ; xmm14 = b or c
    pshufb xmm14,xmm14,paeth_shuffle_mask_16_to_8
                                                ; xmm14 = b or c (8-bit)
    pand xmm12,xmm14                            ; xmm12 = a or b or c
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
    abs si                                      ; si = pa
    mov di,dx
    sub di,bx
    abs di                                      ; di = pb
    sub dx,cx
    abs dx                                      ; dx = pc
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

