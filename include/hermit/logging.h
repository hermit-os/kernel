/*
 * Copyright (c) 2016, Daniel Krebs, RWTH Aachen University
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

#ifndef __HERMIT_LOGGING_H__
#define __HERMIT_LOGGING_H__

#include <hermit/time.h>
#include <hermit/syscall.h>
#include <hermit/stddef.h>

enum {
	LOG_LEVEL_DISABLED = 0,
	LOG_LEVEL_ERROR,
	LOG_LEVEL_WARNING,
	LOG_LEVEL_INFO,
	LOG_LEVEL_DEBUG,
	LOG_LEVEL_VERBOSE
};

#define LOG_LEVEL_ERROR_PREFIX		"ERROR"
#define LOG_LEVEL_WARNING_PREFIX	"WARNING"
#define LOG_LEVEL_INFO_PREFIX		"INFO"
#define LOG_LEVEL_DEBUG_PREFIX		"DEBUG"
#define LOG_LEVEL_VERBOSE_PREFIX	"VERBOSE"

#ifndef LOG_LEVEL
    #define LOG_LEVEL LOG_LEVEL_INFO
#endif

// Gratefully taken from Leushenko @ http://stackoverflow.com/a/19017591
#define CONC(a,b) a##_##b
#define IF(c, t, e) CONC(IF, c)(t, e)
#define IF_0(t, e) e
#define IF_1(t, e) t

#define __LOG_FUNCTION(...) kprintf(__VA_ARGS__)

// [timestamp][core:task][level] ...
#define __LOG_FORMAT_VERBOSE(level, fmt, ...) \
	"[%d.%03d][%d:%d][" CONC(level, PREFIX) "] " fmt, \
	(get_uptime() / 1000), (get_uptime() % 1000), \
	CORE_ID, sys_getpid(), \
	##__VA_ARGS__

// don't add any formatting
#define __LOG_FORMAT_PASS(level, fmt, ...) fmt, ##__VA_ARGS__

// The compiler will optimize the if clause away since the condition can be
// evaluated at compile-time
#define __LOG(level, formatter, ...) do {	\
	if(LOG_LEVEL >= level) {	\
	    __LOG_FUNCTION(formatter(level, __VA_ARGS__));	\
	}	\
} while(0)

#define LOG_ERROR(...)		__LOG(LOG_LEVEL_ERROR, __LOG_FORMAT_VERBOSE, __VA_ARGS__)
#define LOG_WARNING(...)	__LOG(LOG_LEVEL_WARNING, __LOG_FORMAT_VERBOSE, __VA_ARGS__)
#define LOG_INFO(...)		__LOG(LOG_LEVEL_INFO, __LOG_FORMAT_VERBOSE, __VA_ARGS__)
#define LOG_DEBUG(...)		__LOG(LOG_LEVEL_DEBUG, __LOG_FORMAT_VERBOSE, __VA_ARGS__)
#define LOG_VERBOSE(...)	__LOG(LOG_LEVEL_VERBOSE, __LOG_FORMAT_VERBOSE, __VA_ARGS__)

// No formatting will be applied, so this can be used to expand the previous
// line.
#define LOG_SAME_LINE(level, ...)	__LOG(level, __LOG_FORMAT_PASS, __VA_ARGS__)

#endif // __HERMIT_LOGGING_H__
