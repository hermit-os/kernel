; 
; Written by the Chair for Operating Systems, RWTH Aachen University
; 
; NO Copyright (C) 2010-2011, Stefan Lankes
; consider these trivial functions to be public domain.
; 
; These functions are distributed on an "AS IS" BASIS,
; WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
;

%include "config.inc"

%ifdef CONFIG_X86_32
[BITS 32]
%else
[BITS 64]
%endif
SECTION .text
global strcpy
strcpy:
%ifdef CONFIG_X86_32
   push ebp
   mov ebp, esp
   push edi
   push esi

   mov esi, [ebp+12]
   mov edi, [ebp+8]
%else
   push rdi
%endif

L1:
   lodsb
   stosb
   test al, al
   jne L1

%ifdef CONFIG_X86_32
   mov eax, [ebp+8]
   pop esi
   pop edi
   pop ebp
%else
   pop rax
%endif
   ret

global strncpy
strncpy:
%ifdef CONFIG_X86_32
   push ebp
   mov ebp, esp
   push edi
   push esi

   mov ecx, [ebp+16]
   mov esi, [ebp+12]
   mov edi, [ebp+8]

L2:
   dec ecx
%else
   push rdi
   mov rcx, rdx

L2:
   dec rcx
%endif
   js L3
   lodsb
   stosb
   test al, al
   jne L1
   rep
   stosb

L3:
%ifdef CONFIG_X86_32
   mov eax, [ebp+8]
   pop esi
   pop edi
   pop ebp
%else
   pop rax
%endif
   ret

SECTION .note.GNU-stack noalloc noexec nowrite progbits
