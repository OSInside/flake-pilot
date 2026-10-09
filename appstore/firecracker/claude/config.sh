#!/bin/bash

set -ex

ls -l /boot 1>&2

#======================================
# FireCracker wants uncompressed kernel
#--------------------------------------
if [ "$(uname -m)" = "x86_64" ];then
    rm -f /boot/vmlinuz*
    rm -f /lib/modules/*/vmlinuz
    cp /usr/lib/modules/*/vmlinux.xz /boot/vmlinux.xz
    xz -d /boot/vmlinux.xz
    cp /boot/vmlinux /lib/modules/*/
fi

#======================================
# Create host keys
#--------------------------------------
/usr/sbin/sshd-gen-keys-start

zypper ar https://download.opensuse.org/distribution/leap/16.0/repo/oss Leap

npm install -g @anthropic-ai/claude-code@latest

curl https://sdk.cloud.google.com > install.sh
bash install.sh --disable-prompts --install-dir=/usr/share

ln -s /usr/share/google-cloud-sdk/bin/gcloud /usr/bin/gcloud
ln -s /usr/share/google-cloud-sdk/bin/gsutil /usr/bin/gsutil
ln -s /usr/share/google-cloud-sdk/bin/bq /usr/bin/bq

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

chown -R ai:users /home/ai
