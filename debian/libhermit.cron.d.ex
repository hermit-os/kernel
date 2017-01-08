#
# Regular cron jobs for the libhermit package
#
0 4	* * *	root	[ -x /usr/bin/libhermit_maintenance ] && /usr/bin/libhermit_maintenance
