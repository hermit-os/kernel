/*
 * Copyright (c) 2018, Stefan Lankes, RWTH Aachen University
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

#ifndef __UHYVE_H__
#define __UHYVE_H__

#include <err.h>

#define UHYVE_PORT_WRITE		0x400
#define UHYVE_PORT_OPEN			0x440
#define UHYVE_PORT_CLOSE		0x480
#define UHYVE_PORT_READ			0x500
#define UHYVE_PORT_EXIT			0x540
#define UHYVE_PORT_LSEEK		0x580

// Networkports
#define UHYVE_PORT_NETINFO              0x600
#define UHYVE_PORT_NETWRITE             0x640
#define UHYVE_PORT_NETREAD              0x680
#define UHYVE_PORT_NETSTAT              0x700

/* Ports and data structures for uhyve command line arguments and envp
 * forwarding */
#define UHYVE_PORT_CMDSIZE		0x740
#define UHYVE_PORT_CMDVAL		0x780

#define UHYVE_IRQ       11

#define kvm_ioctl(fd, cmd, arg) ({ \
        const int ret = ioctl(fd, cmd, arg); \
        if(ret == -1) \
                err(1, "KVM: ioctl " #cmd " failed"); \
        ret; \
        })

void print_registers(void);
void timer_handler(int signum);
void restore_cpu_state(void);
void save_cpu_state(void);
void init_cpu_state(uint64_t elf_entry);
int load_kernel(uint8_t* mem, char* path);
int load_checkpoint(uint8_t* mem, char* path);
void init_kvm_arch(void);
int load_kernel(uint8_t* mem, char* path);

#endif
