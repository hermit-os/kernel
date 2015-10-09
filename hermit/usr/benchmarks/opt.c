/*
 * =====================================================================================
 *
 *       Filename:  opt.c
 *
 *    Description:  
 *
 *        Version:  1.0
 *        Created:  25.07.2014 15:01:32
 *       Revision:  none
 *       Compiler:  gcc
 *
 *         Author:  Georg Wassen (gw) (), 
 *        Company:  
 *
 * =====================================================================================
 */

#define _GNU_SOURCE
#include "opt.h"

#include <stdio.h>
#include <unistd.h>
#include <stdlib.h>
#include <string.h>
#include <libgen.h>

#ifdef __hermit__
char *basename(char *path)
{
	char *p;
	if( path == NULL || *path == '\0' )
		return ".";
	p = path + strlen(path) - 1;
	while( *p == '/' ) {
		if( p == path )
			return path;
		*p-- = '\0';
	}
	while( p >= path && *p != '/' )
		p--;
	return p + 1;
}
#endif

int opt(int argc, char *argv[], struct opt *opt)
{
    char c;
    char *p;

    opt->secs = 4;
    opt->mode = stat;
    opt->threshold = 0;

    opt->hist_cnt = 100;
    opt->hist_width = 50;

    opt->list_cnt = 1000;


    /*
     * read command line arguments and store them in opt
     */

    while ((c = getopt(argc, argv, "b:c:d:hr:t:")) != -1) {
        switch (c) {
            case 'h' :
                printf("usage: %s <options>\n", basename(argv[0]));
                printf("   -h       help  \n");
                printf("   -d N     duration (in sec or 1m, 1h)  \n");
                printf("   -r R     report: hist, list\n");
                printf("   -c N     count (hist or list)\n");
                printf("   -b N     hist bin width (in ticks)\n");
                printf("   -t N     threshold (in ticks)\n");
                exit(1);
                break;
            case 'd' :
                opt->secs = (unsigned)strtoul(optarg, &p, 0);
                if (p[0] == 'm' || p[0] == 'M') opt->secs *= 60u;
                else if (p[0] == 'h' || p[0] == 'H') opt->secs *= (60u*60u);
                else if (strlen(p) > 0) {
                    printf("ERROR: Parameter: unrecognized characters in time: '%s'\n", p);
                    return 2;
                }
                break;
            case 'r' :
                if (strncmp(optarg, "hist", 4) == 0) {
                    opt->mode = hist;
                } else if (strncmp(optarg, "list", 4) == 0) {
                    opt->mode = list;
                }
                break;
            case 'c' :
                if (opt->mode == hist) {
                    opt->hist_cnt = (unsigned)strtoul(optarg, &p, 0);
                } else if (opt->mode == list) {
                    opt->list_cnt = (unsigned)strtoul(optarg, &p, 0);
                }
                break;
            case 'b' :
                opt->hist_width = (unsigned)strtoul(optarg, &p, 0);
                break;
            case 't' :
                opt->threshold = (unsigned)strtoul(optarg, &p, 0);
                break;
        }
    }

    if (opt->mode == hist) {
        if (opt->hist_cnt == 0) {
            opt->hist_cnt = 1;
        }
        if (opt->hist_width == 0) {
            opt->hist_width = 1;
        }
    } else if (opt->mode == list) {
        if (opt->list_cnt == 0) {
            opt->list_cnt = 1;
        }
    }

    return 0;
}

