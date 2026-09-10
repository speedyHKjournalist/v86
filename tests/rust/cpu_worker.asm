bits 16
org 0
    cli
    lgdt [cs:gdtr]
    mov eax, cr0
    or eax, 1
    mov cr0, eax
    jmp dword 8:0xF0000 + protected
bits 32
protected:
    mov ax, 16
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov fs, ax
    mov gs, ax
    mov esp, 0x8000
    mov dword [0x500], 0xCAFE
.loop:
    inc dword [0x504]
    mov eax, [0x600]
    test eax, eax
    jz .loop
    call eax
    mov dword [0x600], 0
    jmp .loop
align 8
gdt: dq 0, 0x00CF9A000000FFFF, 0x00CF92000000FFFF
gdtr: dw 23
      dd 0xF0000 + gdt
times 0xFFF0 - ($ - $$) db 0xF4
bits 16
    jmp 0xF000:0
times 0x10000 - ($ - $$) db 0xF4
