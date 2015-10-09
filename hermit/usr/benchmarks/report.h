/*
 * =====================================================================================
 *
 *       Filename:  report.h
 *
 *    Description:  
 *
 *        Version:  1.0
 *        Created:  25.07.2014 15:11:07
 *       Revision:  none
 *       Compiler:  gcc
 *
 *         Author:  Georg Wassen (gw) (), 
 *        Company:  
 *
 * =====================================================================================
 */

#ifndef __REPORT_H__
#define __REPORT_H__

#include "run.h"
#include "opt.h"

int report_params(const struct opt *opt);
int report(const struct opt *opt, const struct result *result);

#endif //  __REPORT_H__
