/* 
 * Copyright 2011 Carl-Benedikt Krueger, Chair for Operating Systems,
 *                                       RWTH Aachen University
 *
 * This software is available to you under a choice of one of two
 * licenses.  You may choose to be licensed under the terms of the GNU
 * General Public License (GPL) Version 2 (https://www.gnu.org/licenses/gpl-2.0.txt)
 * or the BSD license below:
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
#include <hermit/logging.h>
#include "util.h"

inline int isprint(unsigned char e)
{
	if ((e < 0x30) || (e > 0x80))
		return 0;
	return 1;
}

// hex_dumb display network packets in a good way
void hex_dump(unsigned n, const unsigned char *buf)
{
	int on_this_line = 0;

	while (n-- > 0)
	{
		LOG_SAME_LINE(LOG_LEVEL_INFO, "%02X ", *buf++);
		on_this_line += 1;

		if (on_this_line == 16 || n == 0)
		{
			int i;

			LOG_SAME_LINE(LOG_LEVEL_INFO, " ");
			for (i = on_this_line; i < 16; i++)
				LOG_SAME_LINE(LOG_LEVEL_INFO, " ");
			for (i = on_this_line; i > 0; i--)
				LOG_SAME_LINE(LOG_LEVEL_INFO, "%c", isprint(buf[-i]) ? buf[-i] : '.');
			LOG_SAME_LINE(LOG_LEVEL_INFO, "\n");
			on_this_line = 0;
		}
	}
}
