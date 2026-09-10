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
    ; Identity-map 8 MiB, plus virtual aliases of the two-page test module.
    cld
    xor eax, eax
    mov edi, 0x12000
    mov ecx, 4096
    rep stosd
    mov dword [0x12000], 0x13003
    mov dword [0x12004], 0x14003
    mov dword [0x12008], 0x15003
    mov edi, 0x13000
    mov eax, 3
    mov ecx, 2048
.pte:
    stosd
    add eax, 4096
    loop .pte
    mov dword [0x15000], 0x100003
    mov dword [0x15004], 0x101003
    mov eax, 0x12000
    mov cr3, eax
    mov eax, cr0
    or eax, 0x80000000
    mov cr0, eax
    mov dword [0x500], 0xCAFE
    hlt
    jmp $
align 8
gdt:
    dq 0, 0x00CF9A000000FFFF, 0x00CF92000000FFFF
gdtr:
    dw 23
    dd 0xF0000 + gdt
times 0xFFF0 - ($ - $$) db 0xF4
bits 16
    jmp 0xF000:start
start equ 0
times 0x10000 - ($ - $$) db 0xF4
