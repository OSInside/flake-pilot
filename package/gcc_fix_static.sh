#!/bin/bash
# This is a hack and related to the issue explained here:
# https://github.com/rust-lang/rust/issues/99382
#
set -ex

if [ ! -e /usr/bin/sudo ]; then
    echo "no sudo available... skipped"
    exit 0
fi
if [ -e /usr/bin/gcc-12.bin ];then
    echo "gcc already wrapped... skipped"
    exit 0
fi
if [ ! -e /usr/bin/gcc-12 ];then
    echo "no gcc-12 system... skipped"
    exit 0
fi
mv /usr/bin/gcc-12 /usr/bin/gcc-12.bin

cat >/usr/bin/gcc-12 <<- EOF
#!/bin/bash
args=\$(echo \$@ | sed -e "s@static-pie@static@")
/usr/bin/gcc-12.bin \$args
EOF

chmod 755 /usr/bin/gcc-12
