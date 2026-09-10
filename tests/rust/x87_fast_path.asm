bits 16
org 0
start:
    cli
    mov ax, 0xF000
    mov ds, ax
    xor ax, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x8000
again:
    mov bp, controls
    mov cx, 12
    mov di, 0x500
.control:
    mov si, operands
    mov bx, (operands_end - operands) / 20
.pair:
%macro calculate 1
    fninit
    fldcw [ds:bp]
    fld tword [si]
    fld tword [si + 10]
    %1 st1, st0
    fnstsw [es:di + 10]
    fstp tword [es:di]
    add di, 12
%endmacro
    calculate faddp
    calculate fsubp
    calculate fmulp
    calculate fdivp
%macro compare 1
    fninit
    fldcw [ds:bp]
    fld tword [si]
    fld tword [si + 10]
    %1 st1
    fnstsw [es:di + 10]
    fstp tword [es:di]
    add di, 12
%endmacro
    compare fcom
    compare fucom
    add si, 20
    dec bx
    jnz .pair
    mov si, single_inputs
    mov bx, (single_inputs_end - single_inputs) / 4
.single_load:
    fninit
    fldcw [ds:bp]
    fld dword [si]
    fnstsw [es:di + 10]
    fst dword [es:di + 12]
    fnstsw [es:di + 16]
    fstp tword [es:di]
    add di, 18
    add si, 4
    dec bx
    jnz .single_load
    mov si, single_store_inputs
    mov bx, (single_store_inputs_end - single_store_inputs) / 10
.single_store:
    fninit
    fldcw [ds:bp]
    fld tword [si]
    fstp dword [es:di]
    fnstsw [es:di + 4]
    add di, 6
    add si, 10
    dec bx
    jnz .single_store
    add bp, 2
    dec cx
    jnz .control
    inc word [es:0x400]
    jmp again
controls:
    dw 0x007F, 0x047F, 0x087F, 0x0C7F
    dw 0x027F, 0x067F, 0x0A7F, 0x0E7F
    dw 0x037F, 0x077F, 0x0B7F, 0x0F7F
operands:
%macro pair 4
    dq %1
    dw %2
    dq %3
    dw %4
%endmacro
    pair 0xC000000000000000, 0x4000, 0x8000000000000000, 0x4000
    pair 0x8000010000000000, 0x3FFF, 0xFFFFFF0000000000, 0xBFFF
    pair 0x8000000000000001, 0x3FFF, 0x8000000000000001, 0xBFFF
    pair 0xFFFFFFFFFFFFFFFF, 0x7FFE, 0x8000000000000000, 0x4000
    pair 0x8000000000000000, 0x0001, 0x8000000000000000, 0x4000
    pair 1, 0, 0x8000000000000000, 0x3FFF
    pair 0, 0x8000, 0, 0
    pair 0x8000000000000000, 0x7FFF, 0x8000000000000000, 0xFFFF
    pair 0x8000000000000001, 0x7FFF, 0xC000000000000001, 0x7FFF
operands_end:
single_inputs:
    dd 0, 0x80000000, 1, 0x80000001, 0x007FFFFF, 0x00800000
    dd 0x3F800000, 0xBF800000, 0x3FC12345, 0x7F7FFFFF, 0xFF7FFFFF
    dd 0x7F800000, 0xFF800000, 0x7FC12345, 0x7F812345, 0xFFC12345
single_inputs_end:
single_store_inputs:
%macro store_input 2
    dq %1
    dw %2
%endmacro
    store_input 0x8000000000000000, 0x3FFF
    store_input 0x8000008000000000, 0x3FFF
    store_input 0x8000018000000000, 0x3FFF
    store_input 0x8000008000000001, 0xBFFF
    store_input 0xFFFFFF8000000000, 0x407E
    store_input 0x8000000000000001, 0x3F80
    store_input 0x8000000000000001, 0x3F69
    store_input 0, 0x8000
    store_input 0x8000000000000000, 0x7FFF
    store_input 0x8000000000000001, 0x7FFF
    store_input 0xC000000000000001, 0xFFFF
    store_input 0x7FFFFFFFFFFFFFFF, 0x3FFF
single_store_inputs_end:
times 0xFFF0 - ($ - $$) db 0xF4
    jmp 0xF000:start
times 0x10000 - ($ - $$) db 0xF4
