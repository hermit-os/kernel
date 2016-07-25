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

#include <hermit/string.h>
#include <asm/io.h>
#include <asm/vga.h>

#define VIDEO_MEM_ADDR	0xB8000 /* the video memory address */

/*
 * These define our textpointer, our background and foreground
 * colors (attributes), and x and y cursor coordinates 
 */
static unsigned short *textmemptr;
static int attrib = 0x0F;
static int csr_x = 0, csr_y = 0;

inline static unsigned short *memsetw(unsigned short *dest, unsigned short val, size_t count)
{
	size_t i;

	if (BUILTIN_EXPECT(!dest, 0))
		return dest;

	for (i = 0; i < count; i++)
		dest[i] = val;

	return dest;
}

/* Scrolls the screen */
static void scroll(void)
{
	unsigned blank, temp;

	/* 
	 * A blank is defined as a space... we need to give it
	 * backcolor too 
	 */
	blank = 0x20 | (attrib << 8);

	/* Row 25 is the end, this means we need to scroll up */
	if (csr_y >= 25) {

		/* 
		 * Move the current text chunk that makes up the screen
		 *  
		 * back in the buffer by one line 
		 */
		temp = csr_y - 25 + 1;
		memcpy(textmemptr, textmemptr + temp * 80,
		       (25 - temp) * 80 * 2);

		/* 
		 * Finally, we set the chunk of memory that occupies
		 * the last line of text to our 'blank' character 
		 */
		memsetw(textmemptr + (25 - temp) * 80, blank, 80);
		csr_y = 25 - 1;
	}
}

/* 
 * Updates the hardware cursor: the little blinking line
 * on the screen under the last character pressed! 
 */
static void move_csr(void)
{
	unsigned temp;

	/* 
	 * The equation for finding the index in a linear
	 * chunk of memory can be represented by:
	 * Index = [(y * width) + x] */
	temp = csr_y * 80 + csr_x;

	/* 
	 * This sends a command to indicies 14 and 15 in the
	 * CRT Control Register of the VGA controller. These
	 * are the high and low bytes of the index that show
	 * where the hardware cursor is to be 'blinking'. To
	 * learn more, you should look up some VGA specific
	 * programming documents. A great start to graphics:
	 * http://www.brackeen.com/home/vga 
	 */
	outportb(0x3D4, 14);
	outportb(0x3D5, temp >> 8);
	outportb(0x3D4, 15);
	outportb(0x3D5, temp);
}

/* Clears the screen */
void vga_clear(void)
{
	unsigned blank;
	int i;

	/*
	 * Again, we need the 'short' that will be used to
	 * represent a space with color 
	 */
	blank = 0x20 | (attrib << 8);

	/* 
	 * Fills the entire screen with spaces in our current
	 * color 
	 **/
	for (i = 0; i < 25; i++)
		memsetw(textmemptr + i * 80, blank, 80);

	/* 
	 * Update out virtual cursor, and then move the
	 * hardware cursor 
	 */
	csr_x = 0;
	csr_y = 0;
	move_csr();
}

/* Puts a single character on the screen */
int vga_putchar(unsigned char c)
{
	unsigned short *where;
	unsigned att = attrib << 8;

	/* Handle a backspace by moving the cursor back one space */
	if (c == 0x08) {
		if (csr_x != 0)
			csr_x--;
	}

	/* 
	 * Handles a tab by incrementing the cursor's x, but only
	 * to a point that will make it divisible by 8 
	 */
	else if (c == 0x09) {
		csr_x = (csr_x + 8) & ~(8 - 1);
	}

	/* 
	 * Handles a 'Carriage Return', which simply brings the
	 * cursor back to the margin 
	 */
	else if (c == '\r') {
		csr_x = 0;
	}

	/* 
	 * We handle our newlines the way DOS and BIOS do: we
	 * treat it as if a 'CR' was there also, so we bring the
	 * cursor to the margin and increment the 'y' value 
	 */
	else if (c == '\n') {
		csr_x = 0;
		csr_y++;
	}

	/* 
	 * Any character greater than and including the space is a
	 * printable character. The equation for finding the index
	 * in a linear chunk of memory can be represented by:
	 * Index = [(y * width) + x] 
	 */
	else if (c >= ' ') {
		where = textmemptr + (csr_y * 80 + csr_x);
		*where = c | att;	/* Character AND attributes: color */
		csr_x++;
	}

	/* 
	 * If the cursor has reached the edge of the screen's width, we
	 * insert a new line in there 
	 */
	if (csr_x >= 80) {
		csr_x = 0;
		csr_y++;
	}

	/* Scroll the screen if needed, and finally move the cursor */
	scroll();
	move_csr();

	return (int) c;
}

/* Uses the routine above to output a string... */
int vga_puts(const char *text)
{
	size_t i;

	for (i = 0; i < strlen(text); i++)
		vga_putchar(text[i]);

	return i-1;
}

/* Sets the forecolor and backcolor we will use */
//void settextcolor(unsigned char forecolor, unsigned char backcolor)
//{

	/* 
	 * Top 4 bytes are the background, bottom 4 bytes
	 * are the foreground color 
	 */
//	attrib = (backcolor << 4) | (forecolor & 0x0F);
//}

/* Sets our text-mode VGA pointer, then clears the screen for us */
void vga_init(void)
{
	textmemptr = (unsigned short *)VIDEO_MEM_ADDR;
	// our bootloader already cleared the screen
	vga_clear();
}
