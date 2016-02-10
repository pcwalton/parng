; parng/interlace.asm

bits 64

global parng_deinterlace_adam7_scanline_04
global parng_deinterlace_adam7_scanline_26

; parng_deinterlace_adam7_scanline_04(uint8x4 *dest,  /* rdi */
;                                     uint8x4 *lod0,  /* rsi; must not be null */
;                                     uint8x4 *lod1,  /* rdx; can be null */
;                                     uint8x4 *lod3,  /* rcx; can be null */
;                                     uint8x4 *lod5,  /* r8; can be null */
;                                     uint64_t width) /* r9 */
;
; TODO(pcwalton): Try specializing this to eliminate the branches.
; TODO(pcwalton): A version of this specialized for scanline 4 would be slightly cheaper.
parng_deinterlace_adam7_scanline_04:
    xorps xmm0,xmm0             ; xmm0 = 0
    xorps xmm1,xmm1             ; xmm1 = 0
    xorps xmm2,xmm2             ; xmm2 = 0
    test rdx,rdx                ; lod1 == null?
    cmove rdx,rsi               ; if so, lod1 = lod0
    xor rax,rax
    xor r10,r10
.loop:
    movd xmm0,[rsi+rax]         ; xmm0 = [ undef, undef, undef, lod0[0] ]
    movd xmm1,[rdx+rax]         ; xmm1 = [ undef, undef, undef, lod1[0] ]
    test rcx,rcx                ; lod3 == null?
    je .lod3_not_present
    pinsrd xmm0,[rcx+rax*2],1   ; xmm0 = [ undef, undef, lod3[0], lod0[0] ]
    pinsrd xmm1,[rcx+rax*2+4],1 ; xmm1 = [ undef, undef, lod3[1], lod1[0] ]
    jmp .lod3_finished
.lod3_not_present:
    pinsrd xmm0,r10d,1          ; xmm0 = [ undef, undef, lod3[0], lod0[0] ]
    pinsrd xmm1,r10d,1          ; xmm1 = [ undef, undef, lod3[0], lod0[0] ]
.lod3_finished:
    test r8,r8                  ; lod5 == null?
    je .lod5_not_present
    unpcklps xmm0,[r8+rax*4]    ; xmm0 = [ lod5[1], lod3[0], lod5[0], lod0[0] ]
    unpckhps xmm1,[r8+rax*4]    ; xmm1 = [ lod5[3], lod3[1], lod5[2], lod1[0] ]
    jmp .lod5_finished
.lod5_not_present:
    unpcklps xmm0,xmm2          ; xmm0 = [ lod5[1], lod3[0], lod5[0], lod0[0] ]
    unpcklps xmm1,xmm2          ; xmm1 = [ lod5[3], lod3[1], lod5[2], lod1[0] ]
.lod5_finished:
    movdqa [rdi+rax*8],xmm0     ; write pixels [0..4)
    movdqa [rdi+rax*8+16],xmm1  ; write pixels [4..8)
    add rax,4
    cmp rax,r9
    jb .loop
    ret

; parng_deinterlace_adam7_scanline_26(uint8x4 *dest, /* rdi */
;                                     uint8x4 *lod4, /* rsi */
;                                     uint8x4 *lod5, /* rdx */
;                                     uint64_t width) /* rcx */
parng_deinterlace_adam7_scanline_26:
    test rdx,rdx                    ; lod5 == null?
    cmove rdx,rsi                   ; if so, lod5 = lod4
    xor rax,rax
.loop:
    movdqa xmm1,[rsi+rax*2]         ; xmm1 = [ lod4[3], lod4[2], lod4[1], lod4[0] ]
    vunpcklps xmm0,xmm1,[rdx+rax*2] ; xmm0 = [ lod5[1], lod4[1], lod5[0], lod4[0] ]
    unpckhps xmm1,[rdx+rax*2]       ; xmm1 = [ lod5[3], lod4[3], lod5[2], lod4[2] ]
    movdqa [rdi+rax*4],xmm0         ; write pixels [0..4)
    movdqa [rdi+rax*4+16],xmm1      ; write pixels [4..8)
    add rax,8
    cmp rax,rcx
    jb .loop
    ret

