/*
 * Copyright (c) 2010-2015, Stefan Lankes, RWTH Aachen University
 * All rights reserved.
 *
 *    * Redistribution and use in source and binary forms, with or without
 *      modification, are permitted provided that the following conditions are met:
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
 * @author Stefan Lankes
 * @file arch/x86/include/asm/pci.h
 * @brief functions related to PCI initialization and information
 *
 * This file contains a procedure to initialize the PCI environment
 * and functions to access information about specific PCI devices.
 */

#ifndef __ARCH_PCI_H__
#define __ARCH_PCI_H__

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
	uint32_t base[6];
	uint32_t size[6];
	uint32_t irq;
} pci_info_t;

#define PCI_IGNORE_SUBID	(0)

/** @brief Initialize the PCI environment
 */
int pci_init(void);

/** @brief Determine the IObase address and the interrupt number of a specific device
 *
 * @param vendor_id The device's vendor ID
 * @param device_id The device's ID
 * @param subystem_id The subsystem DI
 * @param info Pointer to the record pci_info_t where among other the IObase address will be stored
 * @param enable_bus_master If true, the bus mastering will be enabled.
 *
 * @return
 * - 0 on success
 * - -EINVAL (-22) on failure
 */
int pci_get_device_info(uint32_t vendor_id, uint32_t device_id, uint32_t subsystem_id, pci_info_t* info, int8_t enble_bus_master);

/** @brief Print information of existing pci adapters
 *
 * @return 0 in any case
 */
int print_pci_adapters(void);

#ifdef __cplusplus
}
#endif

#endif
