/*
 * =====================================================================================
 *
 *       Filename:  init.c
 *
 *    Description:  
 *
 *        Version:  1.0
 *        Created:  25.07.2014 15:06:05
 *       Revision:  none
 *       Compiler:  gcc
 *
 *         Author:  Georg Wassen (gw) (), 
 *        Company:  
 *
 * =====================================================================================
 */

#include "init.h"
#include "rdtsc.h"


int init(struct opt *opt)
{
    /*
     * initialize (if required)
     * e.g. read number of processors available or cache parameters
     */

    opt->tps = rdtsc_ticks_per_sec();   // does not work reliably...
    //opt->tps = 2530000000; 


    return 0;
}



int deinit(struct opt *opt)
{
    /*
     * de-initialize (if required)
     * e.g. free allocated resources
     */

    return 0;
}
