/*
 * =====================================================================================
 *
 *       Filename:  run.c
 *
 *    Description:  
 *
 *        Version:  1.0
 *        Created:  25.07.2014 15:10:30
 *       Revision:  none
 *       Compiler:  gcc
 *
 *         Author:  Georg Wassen (gw) (), 
 *        Company:  
 *
 * =====================================================================================
 */

#include "run.h"
#include "rdtsc.h"
#include "hist.h"

#include <limits.h>
#include <stdlib.h>

static struct result *results;
static const struct opt *opts;


static void store_results_stat(uint64_t gap, uint64_t offset)
{

    if (gap < results->min) {
        results->min = gap;
        results->t_min = results->cnt;
    }
    if (gap > results->max) {
        results->max = gap;
        results->t_max = results->cnt;
    }
    results->sum += gap;
    results->cnt++;                                 /* avg = sum/cnt */
}

static void store_results_hist(uint64_t gap, uint64_t offset)
{
    /*
     * create histogram 
     */
    store_results_stat(gap, offset);
    hist_add(gap);
}

static unsigned list_cnt;
static unsigned list_idx = 0;
static void store_results_list(uint64_t gap, uint64_t offset)
{
    /*
     * store all timestamps
     */
    if (list_idx >= list_cnt) return;
    store_results_stat(gap, offset);
    results->list[list_idx].time = offset;
    results->list[list_idx].gap = gap;
    list_idx++;
}

static void (*store_results)(uint64_t gap, uint64_t offset);


static int reset_results(void)
{
    results->min=UINT64_MAX;
    results->max=0;
    results->sum=0;
    results->cnt=0;
    results->t_min = 0;
    results->t_max = 0;
    if (opts->mode == hist) {
        hist_reset();
    } else if (opts->mode == list) {
        unsigned i;
        for (i=0; i<opts->list_cnt; i++) {
            results->list[i].time=0;
            results->list[i].gap=0;
        }
        list_idx = 0;
    }
    return 0;
}

static int hourglass(uint64_t duration, uint64_t threshold)
{
    uint64_t t1, t2, t_end, diff;             /* timestamps */

    reset_results();

    rdtsc(&t1);                               /* start-time */
    t_end = t1 + duration;            /* calculate end-time */

    while (t1 < t_end) {             /* loop until end-time */
        t2 = t1;
        rdtsc(&t1);
        diff = t1 - t2;
        if (diff > threshold) {
            store_results(diff, t2);
        }
        /* Note: additional workload may be added here */
    }
    return 0;
}

int run(const struct opt *opt, struct result *result)
{
    unsigned i;
    results = result;
    opts = opt;

    results->hist = NULL;

    switch (opt->mode) {
        case stat :
            store_results = store_results_stat;
            break;
        case hist :
            store_results = store_results_hist;
            results->hist = hist_alloc(opt);
            hist_reset();
            break;
        case list :
            store_results = store_results_list;
            list_cnt = opt->list_cnt;
            results->list = calloc(opt->list_cnt, sizeof(struct res_list));
            for (i=0; i<opt->list_cnt; i++) {
                results->list[i].time=0;
                results->list[i].gap=0;
            }
            break;
    }

    /*
     * execute hourglass routine
     */
    hourglass(1 * opt->tps, opt->threshold); // 1 sec warmup

    hourglass(opt->secs * opt->tps, opt->threshold);
    return 0;
}

int run_free(const struct opt *opt, struct result *result)
{
    if (results->hist != NULL) {
        free(results->hist);
        results->hist = NULL;
    }
    if (results->list != NULL) {
        free(results->list);
        results->list = NULL;
    }
    return 0;
}
