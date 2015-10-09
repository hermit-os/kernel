/*
 * =====================================================================================
 *
 *       Filename:  setup.c
 *
 *    Description:  
 *
 *        Version:  1.0
 *        Created:  25.07.2014 15:07:43
 *       Revision:  none
 *       Compiler:  gcc
 *
 *         Author:  Georg Wassen (gw) (), 
 *        Company:  
 *
 * =====================================================================================
 */

#include "setup.h"


int setup(struct opt *opt)
{
    /*
     * set up run-time environment for benchmark
     * depending on opt
     * e.g. create cpu-set, move IRQs, etc.
     */


    return 0;
}

int setdown(struct opt *opt)
{
    /*
     * undo things from setup()
     */
    return 0;
}
