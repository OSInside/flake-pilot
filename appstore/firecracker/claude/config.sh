#!/bin/bash

set -ex

ls -l /boot 1>&2

#======================================
# FireCracker wants uncompressed kernel
#--------------------------------------
# Delete compressed variants, SUSE provides vmlinux which is
# then taken by kiwi if no other kernel image is present
if [ "$(uname -m)" = "x86_64" ];then
    rm -f /boot/vmlinuz*
    gzip -d /boot/vmlinux*
fi

#======================================
# Create host keys
#--------------------------------------
/usr/sbin/sshd-gen-keys-start

#======================================
# Install claude AI
#--------------------------------------
npm install -g @anthropic-ai/claude-code
