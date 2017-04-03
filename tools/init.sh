#!/bin/sh

PROXY=/hermit/bin/proxy

ELF_OSABI_OFFSET=7
ELF_OSABI="\\x42"

# Network
/sbin/ifconfig lo 127.0.0.1 netmask 255.0.0.0 up
/sbin/ifconfig eth0 up 10.0.2.15 netmask 255.255.255.0 up
/sbin/ifconfig mmnif up 192.168.28.1 netmask 255.255.255.0 up
/sbin/route add default gw 10.0.2.2
/bin/hostname -F /etc/hostname
echo "Network setup completed"

# Load binfmt_misc kernel module and mount pseudo FS
test -d /lib/modules && modprobe binfmt_misc
grep binfmt_misc /proc/mounts || mount binfmt_misc -t binfmt_misc /proc/sys/fs/binfmt_misc

# Register new format
echo ":hermit:M:$ELF_OSABI_OFFSET:$ELF_OSABI::$PROXY:" > /proc/sys/fs/binfmt_misc/register

# Startup completed
sleep 1 && echo -e '\nWelcome to HermitCore (http://www.hermitcore.org/)!'

/bin/sh
