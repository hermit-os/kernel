/*
 * Copyright (c) 2014-2016, Stefan Lankes, Daniel Krebs, RWTH Aachen University
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

#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <page.h>
#include <io.h>
#include <uart.h>

#ifndef CONFIG_VGA

/*
 * This implementation based on following tutorial:
 * http://en.wikibooks.org/wiki/Serial_Programming/8250_UART_Programming
 */

#define UART_RX			0	/* In:  Receive buffer */
#define UART_IIR		2   /* In:  Interrupt ID Register */
#define UART_TX			0	/* Out: Transmit buffer */
#define UART_IER		1	/* Out: Interrupt Enable Register */
#define UART_FCR		2	/* Out: FIFO Control Register */
#define UART_MCR		4	/* Out: Modem Control Register */
#define UART_DLL		0	/* Out: Divisor Latch Low */
#define UART_DLM		1	/* Out: Divisor Latch High */
#define UART_LCR		3	/* Out: Line Control Register */
#define UART_LSR		5	/* Line Status Register */

#define UART_IER_MSI	0x08	/* Enable Modem status interrupt */
#define UART_IER_RLSI	0x04	/* Enable receiver line status interrupt */
#define UART_IER_THRI	0x02	/* Enable Transmitter holding register int. */
#define UART_IER_RDI	0x01	/* Enable receiver data interrupt */

#define UART_IIR_NO_INT		0x01 /* No interrupts pending */
#define UART_IIR_ID			0x06 /* Mask for the interrupt ID */
#define UART_IIR_MSI		0x00 /* Modem status interrupt */
#define UART_IIR_THRI		0x02 /* Transmitter holding register empty */
#define UART_IIR_RDI		0x04 /* Receiver data interrupt */
#define UART_IIR_RLSI		0x06 /* Receiver line status interrupt */

#define UART_FCR_ENABLE_FIFO	0x01 /* Enable the FIFO */
#define UART_FCR_CLEAR_RCVR		0x02 /* Clear the RCVR FIFO */
#define UART_FCR_CLEAR_XMIT		0x04 /* Clear the XMIT FIFO */
#define UART_FCR_TRIGGER_MASK	0xC0 /* Mask for the FIFO trigger range */
#define UART_FCR_TRIGGER_1		0x00 /* Trigger RDI at FIFO level  1 byte */
#define UART_FCR_TRIGGER_4		0x40 /* Trigger RDI at FIFO level  4 byte */
#define UART_FCR_TRIGGER_8		0x80 /* Trigger RDI at FIFO level  8 byte */
#define UART_FCR_TRIGGER_14		0xc0 /* Trigger RDI at FIFO level 14 byte*/


#define UART_LCR_DLAB		0x80 /* Divisor latch access bit */
#define UART_LCR_SBC		0x40 /* Set break control */
#define UART_LCR_SPAR		0x20 /* Stick parity (?) */
#define UART_LCR_EPAR		0x10 /* Even parity select */
#define UART_LCR_PARITY		0x08 /* Parity Enable */
#define UART_LCR_STOP		0x04 /* Stop bits: 0=1 bit, 1=2 bits */
#define UART_LCR_WLEN8		0x03 /* Wordlength: 8 bits */

#define UART_MCR_CLKSEL		0x80 /* Divide clock by 4 (TI16C752, EFR[4]=1) */
#define UART_MCR_TCRTLR		0x40 /* Access TCR/TLR (TI16C752, EFR[4]=1) */
#define UART_MCR_XONANY		0x20 /* Enable Xon Any (TI16C752, EFR[4]=1) */
#define UART_MCR_AFE		0x20 /* Enable auto-RTS/CTS (TI16C550C/TI16C750) */
#define UART_MCR_LOOP		0x10 /* Enable loopback test mode */
#define UART_MCR_OUT2		0x08 /* Out2 complement */
#define UART_MCR_OUT1		0x04 /* Out1 complement */
#define UART_MCR_RTS		0x02 /* RTS complement */
#define UART_MCR_DTR		0x01 /* DTR complement */

#define DEFAULT_UART_PORT	0 //0xc110

size_t	uartport = 0;

static inline unsigned char read_from_uart(uint32_t off)
{
	uint8_t c = 0;

	if (uartport)
		c = inportb(uartport + off);

	return c;
}

static inline int is_transmit_empty(void)
{
	if (uartport)
		return inportb(uartport + UART_LSR) & 0x20;

	return 1;
}

static void write_to_uart(uint32_t off, unsigned char c)
{
	while (is_transmit_empty() == 0) ;

	if (uartport)
		outportb(uartport + off, c);
}

/* Puts a single character on a serial device */
int uart_putchar(unsigned char c)
{
	if (!uartport)
		return 0;

	write_to_uart(UART_TX, c);

	return (int) c;
}

/* Uses the routine above to output a string... */
int uart_puts(const char *text)
{
	size_t i, len = strlen(text);

	if (!uartport)
		return 0;

	for (i = 0; i < len; i++)
		uart_putchar(text[i]);

	return len;
}

static int uart_config(void)
{
	if (!uartport)
		return 0;

	/* disable interrupts */
	write_to_uart(UART_IER, 0);

	/*
	 * 8bit word length
	 * 1 stop bit
	 * no partity
	 * set DLAB=1
	 */
	char lcr = UART_LCR_WLEN8;
	write_to_uart(UART_LCR, lcr);
	lcr = read_from_uart(UART_LCR) | UART_LCR_DLAB;
	write_to_uart(UART_LCR, lcr);

	/*
	 * set baudrate to 38400
	 */
	write_to_uart(UART_DLL, 0x03);
	write_to_uart(UART_DLM, 0x00);

	/* set DLAB=0 */
	write_to_uart(UART_LCR, lcr & (~UART_LCR_DLAB));

	/*
	 * enable FIFOs
	 * clear RX and TX FIFO
	 * set irq trigger to 8 bytes
	 */
	write_to_uart(UART_FCR, UART_FCR_ENABLE_FIFO | UART_FCR_CLEAR_RCVR | UART_FCR_CLEAR_XMIT | UART_FCR_TRIGGER_1);

	return 0;
}

int uart_init(const char* cmdline)
{
	char* str;

	if (!uartport && cmdline && ((str = strstr(cmdline, "uart=io:")) != NULL))
		uartport = strtol(str+8, (char **)NULL, 16);

	if (!uartport)
		uartport = DEFAULT_UART_PORT;

	if (!uartport)
		return 0;

	// configure uart
	return uart_config();
}

#endif
