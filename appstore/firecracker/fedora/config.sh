#!/bin/bash

set -ex

#======================================
# FireCracker wants uncompressed kernel
#--------------------------------------
# Fedora does not provide a vmlinux, only a zstd compressed
# vmlinuz (bzImage). Extract the ELF vmlinux from it and delete
# the compressed variants such that kiwi takes the vmlinux
function extract_vmlinux {
    local image=$1
    local output=$2
    local pos
    # find the zstd magic (28 b5 2f fd) and try to decompress from there
    for pos in $(
        tr '(\265/\375\n' '\nxxx=' < "${image}" | grep -abo '^xxx'
    ); do
        pos=${pos%%:*}
        tail -c+"${pos}" "${image}" | zstd -dcq > "${output}" 2>/dev/null || :
        if readelf -h "${output}" &>/dev/null; then
            return 0
        fi
    done
    rm -f "${output}"
    return 1
}

if [ "$(uname -m)" = "x86_64" ];then
    for kernel in /usr/lib/modules/*/vmlinuz; do
        moddir=$(dirname "${kernel}")
        extract_vmlinux "${kernel}" "${moddir}/vmlinux"
        rm -f "${kernel}"
    done
    rm -f /boot/vmlinuz* /boot/.vmlinuz*
fi

# drop online target services
# for some reason they don't work on static IP assignments
rm /usr/lib/dracut/modules.d/*systemd-networkd/99-wait-online-dracut.conf
rm /usr/lib/dracut/modules.d/*network-manager/nm-wait-online-initrd.service
rm /usr/lib/dracut/modules.d/*network-manager/NetworkManager-wait-online-initrd-dracut.conf

ssh-keygen -A
