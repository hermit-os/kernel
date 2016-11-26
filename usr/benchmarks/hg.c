/*
 * =====================================================================================
 *
 *       Filename:  main.c
 *
 *    Description:  
 *
 *        Version:  1.0
 *        Created:  25.07.2014 14:59:20
 *       Revision:  none
 *       Compiler:  gcc
 *
 *         Author:  Georg Wassen (gw) (), 
 *        Company:  
 *
 * =====================================================================================
 */

#include "opt.h"
#include "init.h"
#include "setup.h"
#include "run.h"
#include "report.h"

#include <stdio.h>
#include <stdlib.h>

struct opt opts = {0};
struct result results = {0};

int main(int argc, char *argv[])
{
    printf("hourglass\n");

    opt(argc, argv, &opts);
    init(&opts);

    report_params(&opts);

    setup(&opts);
    run(&opts, &results);
    setdown(&opts);

    report(&opts, &results);

    run_free(&opts, &results);

    deinit(&opts);

    return EXIT_SUCCESS;
}
