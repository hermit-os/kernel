/*
 * Copyright (c) 2010, Stefan Lankes, RWTH Aachen University
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions are met:
 *    * Redistributions of source code must retain the above copyright
 *      notice, this list of conditions and the following disclaimer.
 *    * Redistributions in binary form must reproduce the above copyright
 *      notice, this list of conditions and the following disclaimer in the
 *      documentation and/or other materials provided with the distribution.
 *    * Neither the name of the University nor the names of its contributors
 *      may be used to endorse or promote products derived from this
 *      software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED. IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE FOR ANY
 * DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
 * LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND
 * ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
 * SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */

/**
 *
 * This file keeps define constants for identification and definition of ELF files.\n
 * ELF files consist of up to five parts:
 * - ELF header
 * - program header table
 * - section header table
 * - ELF sections
 * - ELF segment
 */
#ifndef __ELF_H__
#define __ELF_H__

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

#define ELF_MAGIC 0x464C457F

#define ELF_ET_NONE	0x0000	// no type
#define ELF_ET_REL	0x0001	// relocatable
#define ELF_ET_EXEC	0x0002	// executeable
#define ELF_ET_DYN	0x0003	// Shared-Object-File
#define ELF_ET_CORE	0x0004	// Corefile
#define ELF_ET_LOPROC	0xFF00	// Processor-specific
#define ELF_ET_HIPROC	0x00FF	// Processor-specific
 
#define ELF_EM_NONE	0x0000	// no type
#define ELF_EM_M32	0x0001	// AT&T WE 32100
#define ELF_EM_SPARC	0x0002	// SPARC
#define ELF_EM_386	0x0003	// Intel 80386
#define ELF_EM_68K	0x0004	// Motorola 68000
#define ELF_EM_88K	0x0005	// Motorola 88000
#define ELF_EM_860	0x0007	// Intel 80860
#define ELF_EM_MIPS	0x0008	// MIPS RS3000
#define ELF_EM_X86_64 0x003e // Intel X86_64

#define ELF_CLASS_NONE	0x0000
#define ELF_CLASS_32	0x0001	// 32bit file
#define ELF_CLASS_64	0x0002	// 64bit file
 
#define ELF_DATA_NONE	0x0000
#define ELF_DATA_2LSB	0x0001
#define ELF_DATA_2MSB	0x002
 
/* Legal values for p_type (segment type).  */

#define ELF_PT_NULL		0	/* Program header table entry unused */
#define ELF_PT_LOAD		1	/* Loadable program segment */
#define ELF_PT_DYNAMIC		2	/* Dynamic linking information */
#define ELF_PT_INTERP		3	/* Program interpreter */
#define ELF_PT_NOTE		4	/* Auxiliary information */
#define ELF_PT_SHLIB		5
#define ELF_PT_PHDR		6	/* Entry for header table itself */
#define ELF_PT_TLS		7	/* Thread-local storage segment */
#define ELF_PT_NUM		8	/* Number of defined types */
#define ELF_PT_LOOS		0x60000000	/* Start of OS-specific */
#define ELF_PT_GNU_EH_FRAME	0x6474e550	/* GCC .eh_frame_hdr segment */
#define ELF_PT_GNU_STACK	0x6474e551	/* Indicates stack executability */
#define ELF_PT_GNU_RELRO	0x6474e552	/* Read-only after relocation */
#define ELF_PT_LOSUNW		0x6ffffffa
#define ELF_PT_SUNWBSS		0x6ffffffa	/* Sun Specific segment */
#define ELF_PT_SUNWSTACK	0x6ffffffb	/* Stack segment */
#define ELF_PT_HISUNW		0x6fffffff
#define ELF_PT_HIOS		0x6fffffff	/* End of OS-specific */
#define ELF_PT_LOPROC		0x70000000	/* Start of processor-specific */
#define ELF_PT_HIPROC		0x7fffffff	/* End of processor-specific */

/* These constants define the permissions on sections in the program
   header, p_flags. */
#define PF_R			0x4
#define PF_W			0x2
#define PF_X			0x1

/** @brief Identification part of an ELF-file's header
 *
 * This structure keeps information about the file format
 */
typedef struct {
	uint32_t magic;
	uint8_t _class;
	uint8_t data;
	uint8_t version;
	uint8_t pad[8];
	uint8_t nident;
} __attribute__ ((packed)) elf_ident_t;

/** @brief Information about the executable
 *
 * ELF header\n
 * This structure keeps information about the format of the executable itself.
 */
typedef struct {
	elf_ident_t ident;
	uint16_t type;
	uint16_t machine;
	uint32_t version;
	size_t entry;
	size_t ph_offset;
	size_t sh_offset;
	uint32_t flags;
	uint16_t header_size;
	uint16_t ph_entry_size;
	uint16_t ph_entry_count;
	uint16_t sh_entry_size;
	uint16_t sh_entry_count;
	uint16_t sh_str_table_index;
} __attribute__ ((packed)) elf_header_t;

/** @brief program header information
 *
 * program header table\n
 * This structure keeps information about the program header.
 */
typedef struct
{
	uint32_t	type;
	uint32_t	flags;
	uint64_t	offset;
	uint64_t	virt_addr;
	uint64_t	phys_addr;
	uint64_t	file_size;
	uint64_t	mem_size;
	uint64_t	alignment;
} __attribute__ ((packed)) elf_program_header_t;

/** @brief Information about ELF section
 *
 * ELF section\n
 * This structure keeps information about a specific ELF section
 */
typedef struct {
	uint32_t name;
	uint32_t type;
	uint64_t flags;
	uint64_t addr;
	uint64_t offset;
	uint64_t size;
	uint32_t link;
	uint32_t info;
	uint64_t align;
	uint64_t enttry_size;
} __attribute__ ((packed)) elf_section_header_t;

#ifdef __cplusplus
}
#endif
#endif
