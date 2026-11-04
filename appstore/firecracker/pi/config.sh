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
    rm -f /lib/modules/*/vmlinuz
    cp /usr/lib/modules/*/vmlinux.xz /boot/vmlinux.xz
    xz -d /boot/vmlinux.xz
    cp /boot/vmlinux /lib/modules/*/
fi

/usr/sbin/sshd-gen-keys-start

cat > /home/ai/.alias <<'EOF'
alias ll='ls -lhv'
EOF

cat > /home/ai/.bashrc <<'EOF'
test -s ~/.alias && . ~/.alias || true

export TERM=xterm-256color

source /etc/profile.d/bash-git-prompt.sh

GIT_PROMPT_ONLY_IN_REPO=0
GIT_PROMPT_THEME=Crunch

export PATH="$HOME/.local/bin:$PATH"

pushd /home/ai
EOF

(
su ai

mkdir /home/ai/shared

export HOME=/home/ai
export PATH=$HOME/.local/bin:$PATH

curl -fsSL https://pi.dev/install.sh | sh
curl -fsSL https://pi-bolt.opensec.in/install.sh | sh

pi install npm:@raidou/pi-notify
pi install npm:pi-open-tui
pi install npm:@inobit/pi-themes
pi install npm:@twogiants/pi-anthropic-vertex
pi install npm:@monotykamary/pi-retry
pi install npm:pi-llama-chooser
pi install npm:@narumitw/pi-plan-mode
pi install npm:@juicesharp/rpiv-ask-user-question
pi install npm:@juicesharp/rpiv-todo
pi install npm:pi-lens
pi install npm:@dietrichgebert/ponytail
pi install npm:pi-web-access
)

chown -R ai:users /home/ai
