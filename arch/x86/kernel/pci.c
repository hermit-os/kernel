/*
 * Copyright (c) 2010-2015, Stefan Lankes, RWTH Aachen University
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

#include <hermit/stdio.h>
#include <hermit/string.h>
#include <hermit/errno.h>
#include <hermit/logging.h>
#include <asm/irqflags.h>
#include <asm/io.h>

#include <asm/pci.h>
#ifdef WITH_PCI_IDS
#include "pcihdr.h"
#endif

/*
 * PCI configuration registers
 */
#define	PCI_CFID	0x00	/* Configuration ID */
#define	PCI_CFCS	0x04	/* Configurtion Command/Status */
#define	PCI_CFRV	0x08	/* Configuration Revision */
#define	PCI_CFLT	0x0c	/* Configuration Latency Timer */
#define	PCI_CBIO	0x10	/* Configuration Base IO Address */
#define PCI_CSID	0x2C	/* Configuration Subsystem Id & Subsystem Vendor Id */
#define	PCI_CFIT	0x3c	/* Configuration Interrupt */
#define	PCI_CFDA	0x40	/* Configuration Driver Area */

#define PHYS_IO_MEM_START	0
#define	PCI_MEM			0
#define	PCI_INTA		0
#define PCI_NSLOTS		22
#define PCI_NBUS		0

#define	PCI_CONF_ADDR_REG	0xcf8
#define	PCI_CONF_FRWD_REG	0xcf8
#define	PCI_CONF_DATA_REG	0xcfc

#define PCI_IO_CONF_START	0xc000

#define MAX_BUS			16
#define MAX_SLOTS		32

static uint32_t mechanism = 0;
static uint32_t adapters[MAX_BUS][MAX_SLOTS] = {[0 ... MAX_BUS-1][0 ... MAX_SLOTS-1] = -1};

static void pci_conf_write(uint32_t bus, uint32_t slot, uint32_t off, uint32_t val)
{
	if (mechanism == 1) {
		outportl(PCI_CONF_FRWD_REG, bus);
		outportl(PCI_CONF_ADDR_REG, 0xf0);
		outportl(PCI_IO_CONF_START | (slot << 8) | off, val);
	} else {
		outportl(PCI_CONF_ADDR_REG,
		      (0x80000000 | (bus << 16) | (slot << 11) | off));
		outportl(PCI_CONF_DATA_REG, val);
	}
}

static uint32_t pci_conf_read(uint32_t bus, uint32_t slot, uint32_t off)
{
	uint32_t data = -1;

	outportl(PCI_CONF_ADDR_REG,
	      (0x80000000 | (bus << 16) | (slot << 11) | off));
	data = inportl(PCI_CONF_DATA_REG);

	if ((data == 0xffffffff) && (slot < 0x10)) {
		outportl(PCI_CONF_FRWD_REG, bus);
		outportl(PCI_CONF_ADDR_REG, 0xf0);
		data = inportl(PCI_IO_CONF_START | (slot << 8) | off);
		if (data == 0xffffffff)
			return data;
		if (!mechanism)
			mechanism = 1;
	} else if (!mechanism)
		mechanism = 2;

	return data;
}

static inline uint32_t pci_subid(uint32_t bus, uint32_t slot)
{
	return pci_conf_read(bus, slot, PCI_CSID);
}

static inline uint32_t pci_what_irq(uint32_t bus, uint32_t slot)
{
	return pci_conf_read(bus, slot, PCI_CFIT) & 0xFF;
}

static inline uint32_t pci_what_iobase(uint32_t bus, uint32_t slot, uint32_t nr)
{
	return pci_conf_read(bus, slot, PCI_CBIO + nr*4) & 0xFFFFFFFC;
}

static inline void pci_bus_master(uint32_t bus, uint32_t slot)
{
	// set the device to a bus master

	uint32_t cmd = pci_conf_read(bus, slot, PCI_CFCS) | 0x4;
	pci_conf_write(bus, slot, PCI_CFCS, cmd);
}

static inline uint32_t pci_what_size(uint32_t bus, uint32_t slot, uint32_t nr)
{
	uint32_t tmp, ret;

	// backup the original value
	tmp = pci_conf_read(bus, slot, PCI_CBIO + nr*4);

	// determine size
	pci_conf_write(bus, slot, PCI_CBIO + nr*4, 0xFFFFFFFF);
	ret = ~pci_conf_read(bus, slot, PCI_CBIO + nr*4) + 1;

	// restore original value
	pci_conf_write(bus, slot, PCI_CBIO + nr*4, tmp);

	return ret;
}

int pci_init(void)
{
	uint32_t slot, bus;

	for (bus = 0; bus < MAX_BUS; bus++)
		for (slot = 0; slot < MAX_SLOTS; slot++)
			adapters[bus][slot] = pci_conf_read(bus, slot, PCI_CFID);

	return 0;
}

int pci_get_device_info(uint32_t vendor_id, uint32_t device_id, uint32_t subsystem_id, pci_info_t* info, int8_t bus_master)
{
	uint32_t slot, bus, i;

	if (!info)
		return -EINVAL;

	if (!mechanism && !is_uhyve())
		pci_init();

	for (bus = 0; bus < MAX_BUS; bus++) {
		for (slot = 0; slot < MAX_SLOTS; slot++) {
			if (adapters[bus][slot] != -1) {
				if (((adapters[bus][slot] & 0xffff) == vendor_id) &&
				   (((adapters[bus][slot] & 0xffff0000) >> 16) == device_id) &&
				   (((pci_subid(bus, slot) >> 16) & subsystem_id) == subsystem_id)) {
					for(i=0; i<6; i++) {
						info->base[i] = pci_what_iobase(bus, slot, i);
						info->size[i] = (info->base[i]) ? pci_what_size(bus, slot, i) : 0;
					}
					info->irq = pci_what_irq(bus, slot);
					if (bus_master)
						pci_bus_master(bus, slot);
					return 0;
				}
			}
		}
	}

	return -EINVAL;
}

int print_pci_adapters(void)
{
	uint32_t slot, bus;
	uint32_t counter = 0;
#ifdef WITH_PCI_IDS
	uint32_t i;
#endif

	if (!mechanism)
		pci_init();

	for (bus = 0; bus < MAX_BUS; bus++) {
                for (slot = 0; slot < MAX_SLOTS; slot++) {

		if (adapters[bus][slot] != -1) {
				counter++;
				LOG_INFO("%d) Vendor ID: 0x%x  Device Id: 0x%x\n",
					counter, adapters[bus][slot] & 0xffff,
					(adapters[bus][slot] & 0xffff0000) >> 16);

#ifdef WITH_PCI_IDS
				for (i=0; i<PCI_VENTABLE_LEN; i++) {
					if ((adapters[bus][slot] & 0xffff) ==
					    (uint32_t)PciVenTable[i].VenId)
						LOG_INFO("\tVendor is %s\n",
							PciVenTable[i].VenShort);
				}

				for (i=0; i<PCI_DEVTABLE_LEN; i++) {
					if ((adapters[bus][slot] & 0xffff) ==
					    (uint32_t)PciDevTable[i].VenId) {
						if (((adapters[bus][slot] & 0xffff0000) >> 16) ==
						    PciDevTable[i].DevId) {
							LOG_INFO
							    ("\tChip: %s ChipDesc: %s\n",
							     PciDevTable[i].Chip,
							     PciDevTable[i].ChipDesc);
						}
					}
				}
#endif
			}
		}
	}

	return 0;
}
