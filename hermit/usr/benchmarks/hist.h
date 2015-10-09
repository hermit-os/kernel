/*
 * =====================================================================================
 *
 *       Filename:  hist.h
 *
 *    Description:  
 *
 *        Version:  1.0
 *        Created:  26.07.2014 20:02:48
 *       Revision:  none
 *       Compiler:  gcc
 *
 *         Author:  Georg Wassen (gw) (), 
 *        Company:  
 *
 * =====================================================================================
 */

#ifndef __HIST_H__
#define __HIST_H__

#include "opt.h"

#include <stdint.h>

uint32_t *hist_alloc(const struct opt *opt);
int hist_reset(void);
void hist_add(uint64_t t);
int hist_print(void);

#endif // __HIST_H__
