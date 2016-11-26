/*
 * =====================================================================================
 *
 *       Filename:  hist.c
 *
 *    Description:  
 *
 *        Version:  1.0
 *        Created:  26.07.2014 20:02:34
 *       Revision:  none
 *       Compiler:  gcc
 *
 *         Author:  Georg Wassen (gw) (), 
 *        Company:  
 *
 * =====================================================================================
 */

#include "hist.h"

#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <math.h>

static const struct opt *opts;
static uint32_t *hists;

uint32_t *hist_alloc(const struct opt *opt)
{
    opts = opt;
    hists = calloc(opt->hist_cnt, sizeof(uint32_t)); 
    hist_reset();
    return hists;
}

int hist_reset(void)
{
    unsigned i;
    for (i=0; i<opts->hist_cnt; i++) {
        hists[i] = 0;
    }
    return 0;
}

void hist_add(uint64_t t)
{
    t /= opts->hist_width;
    if (t > opts->hist_cnt-1) t = opts->hist_cnt-1;
    hists[t]++;
}

int hist_print(void)
{
    unsigned i;
    unsigned max=0;
    const size_t bar_width = 30;
    char bar[bar_width+1];

    for (i=0; i<opts->hist_cnt; i++) {
        if (hists[i] > max) max = hists[i];
    }
    max = (unsigned)ceil(log10((double)max));
    if (max == 0) max = 1;
    memset(bar, '*', bar_width);
    bar[bar_width] = 0;

    printf("Histogram (%u bins with %u ticks each)\n", opts->hist_cnt, opts->hist_width);
    for (i=0; i<opts->hist_cnt; i++) {
        printf("     %5u : %5u..%5u : %-10u  %s\n", i,
                (unsigned)(i*opts->hist_width),
                (unsigned)((i+1)*opts->hist_width-1),
                (unsigned)(hists[i]),
                (char*)bar+(unsigned)(bar_width-((log10(hists[i]+1.)*bar_width)/max)));
    }
    return 0;
}

