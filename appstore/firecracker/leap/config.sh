#!/bin/bash

set -ex

test -f /.kconfig && . /.kconfig

ls -l /boot 1>&2

#======================================
# FireCracker wants uncompressed kernel
#--------------------------------------
# Delete compressed variants, SUSE provides vmlinux which is
# then taken by kiwi of no other kernel image is present
if [ "$(uname -m)" = "x86_64" ];then
    rm -f /boot/vmlinuz*
    gzip -d /boot/vmlinux*
fi

/usr/sbin/sshd-gen-keys-start

zypper ar https://download.opensuse.org/distribution/leap/16.0/repo/oss Leap
