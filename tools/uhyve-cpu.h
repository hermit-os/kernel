#ifndef __UHYVE_CPU_H__
#define __UHYVE_CPU_H__

#ifndef _BITUL

#ifdef __ASSEMBLY__
#define _AC(X,Y)	X
#define _AT(T,X)	X
#else
#define __AC(X,Y)	(X##Y)
#define _AC(X,Y)	__AC(X,Y)
#define _AT(T,X)	((T)(X))
#endif

#define _BITUL(x)	(_AC(1,UL) << (x))
#define _BITULL(x)	(_AC(1,ULL) << (x))

#endif

/*
 * EFLAGS bits
 */
#define X86_EFLAGS_CF	0x00000001 /* Carry Flag */

/*
 * Basic CPU control in CR0
 */
#define X86_CR0_PE_BIT		0 /* Protection Enable */
#define X86_CR0_PE		_BITUL(X86_CR0_PE_BIT)
#define X86_CR0_PG_BIT		31 /* Paging */
#define X86_CR0_PG		_BITUL(X86_CR0_PG_BIT)

/*
 * Intel CPU features in CR4
 */
#define X86_CR4_PAE_BIT		5 /* enable physical address extensions */
#define X86_CR4_PAE		_BITUL(X86_CR4_PAE_BIT)

/*
 * Intel long mode page directory/table entries
 */
#define X86_PDPT_P_BIT          0 /* Present */
#define X86_PDPT_P              _BITUL(X86_PDPT_P_BIT)
#define X86_PDPT_RW_BIT         1 /* Writable */
#define X86_PDPT_RW             _BITUL(X86_PDPT_RW_BIT)
#define X86_PDPT_PS_BIT         7 /* Page size */
#define X86_PDPT_PS             _BITUL(X86_PDPT_PS_BIT)

/*
 * GDT and KVM segment manipulation
 */

#define GDT_DESC_OFFSET(n) ((n) * 0x8)

#define GDT_GET_BASE(x) (                      \
    (((x) & 0xFF00000000000000) >> 32) |       \
    (((x) & 0x000000FF00000000) >> 16) |       \
    (((x) & 0x00000000FFFF0000) >> 16))

#define GDT_GET_LIMIT(x) (__u32)(                                      \
                                 (((x) & 0x000F000000000000) >> 32) |  \
                                 (((x) & 0x000000000000FFFF)))

/* Constructor for a conventional segment GDT (or LDT) entry */
/* This is a macro so it can be used in initializers */
#define GDT_ENTRY(flags, base, limit)               \
    ((((base)  & _AC(0xff000000, ULL)) << (56-24)) | \
     (((flags) & _AC(0x0000f0ff, ULL)) << 40) |      \
     (((limit) & _AC(0x000f0000, ULL)) << (48-16)) | \
     (((base)  & _AC(0x00ffffff, ULL)) << 16) |      \
     (((limit) & _AC(0x0000ffff, ULL))))

#define GDT_GET_G(x)   (__u8)(((x) & 0x0080000000000000) >> 55)
#define GDT_GET_DB(x)  (__u8)(((x) & 0x0040000000000000) >> 54)
#define GDT_GET_L(x)   (__u8)(((x) & 0x0020000000000000) >> 53)
#define GDT_GET_AVL(x) (__u8)(((x) & 0x0010000000000000) >> 52)
#define GDT_GET_P(x)   (__u8)(((x) & 0x0000800000000000) >> 47)
#define GDT_GET_DPL(x) (__u8)(((x) & 0x0000600000000000) >> 45)
#define GDT_GET_S(x)   (__u8)(((x) & 0x0000100000000000) >> 44)
#define GDT_GET_TYPE(x)(__u8)(((x) & 0x00000F0000000000) >> 40)

#define GDT_TO_KVM_SEGMENT(seg, gdt_table, sel) \
    do {                                        \
        __u64 gdt_ent = gdt_table[sel];         \
        seg.base = GDT_GET_BASE(gdt_ent);       \
        seg.limit = GDT_GET_LIMIT(gdt_ent);     \
        seg.selector = sel * 8;                 \
        seg.type = GDT_GET_TYPE(gdt_ent);       \
        seg.present = GDT_GET_P(gdt_ent);       \
        seg.dpl = GDT_GET_DPL(gdt_ent);         \
        seg.db = GDT_GET_DB(gdt_ent);           \
        seg.s = GDT_GET_S(gdt_ent);             \
        seg.l = GDT_GET_L(gdt_ent);             \
        seg.g = GDT_GET_G(gdt_ent);             \
        seg.avl = GDT_GET_AVL(gdt_ent);         \
    } while (0)

#endif
