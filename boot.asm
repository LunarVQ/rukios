; boot.asm - a simple bootloader that transitions to 32-bit protected mode
[org 0x7C00]
[bits 16]

start:
  cli
  xor ax, ax
  mov ds, ax
  mov es, ax
  mov ss, ax
  mov sp, 0x7C00

  mov si, msg
  call print_str

; Enable A20 line
  in al, 0x92
  or al, 2
  out 0x92, al

; Load GDT
  lgdt [gdt_descriptor]

; Enter protected mode
  mov eax, cr0
  or al, 1
  mov cr0, eax

; Far jump to flush pipeline and switch to 32-bit mode
  jmp 0x08:protected_mode

[bits 32]
protected_mode:
  ; Set up segment registers for 32-bit mode
  mov ax, 0x10      ; Data segment selector
  mov ds, ax
  mov es, ax
  mov fs, ax
  mov gs, ax
  mov ss, ax

  ; Set up stack
  mov esp, 0x90000

  ; Clear screen
  mov edi, 0xB8000
  mov ecx, 2000
  mov eax, 0x07200720  ; Black background, white text, space character
  rep stosd

  ; Print message in protected mode
  mov edi, 0xB8000
  mov esi, protected_msg
  call print_string_32

  ; Halt
  hlt

; 32-bit print function
print_string_32:
  pusha
.loop:
  lodsb
  or al, al
  jz .done
  mov ah, 0x07      ; White text on black background
  mov [edi], ax
  add edi, 2
  jmp .loop
.done:
  popa
  ret

; 16-bit print function
print_str:
  lodsb
  or al, al
  jz .ret
  mov ah, 0x0E
  int 0x10
  jmp print_str
.ret:
  ret

; GDT
gdt:
  ; Null descriptor
  dq 0
  
  ; Code segment descriptor
  dw 0xFFFF      ; Limit (bits 0-15)
  dw 0x0000      ; Base (bits 0-15)
  db 0x00        ; Base (bits 16-23)
  db 10011010b   ; Access byte
  db 11001111b   ; Flags + Limit (bits 16-19)
  db 0x00        ; Base (bits 24-31)
  
  ; Data segment descriptor
  dw 0xFFFF      ; Limit (bits 0-15)
  dw 0x0000      ; Base (bits 0-15)
  db 0x00        ; Base (bits 16-23)
  db 10010010b   ; Access byte
  db 11001111b   ; Flags + Limit (bits 16-19)
  db 0x00        ; Base (bits 24-31)

gdt_end:

gdt_descriptor:
  dw gdt_end - gdt - 1  ; GDT size
  dd gdt                ; GDT address

msg db "Entering protected mode...", 0
protected_msg db "Hello from 32-bit protected mode!", 0

times 510 - ($ - $$) db 0
dw 0xAA55
