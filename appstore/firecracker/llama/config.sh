#!/bin/bash

set -ex

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

# Install latest
# curl -LsSf https://llama.app/install.sh | sh

# Find models at
# https://huggingface.co/models?library=gguf
mkdir -p models
pushd models
wget https://huggingface.co/ccore/Qwen3.5-4B-3D-Coder/resolve/main/Qwen3.5-4B-3D-Coder-Q4_K_M.gguf

pip3 install --break-system-packages llama-cpp-python
)

chown -R ai:users /home/ai

#======================================
# Allow the ai user to use sudo
#--------------------------------------
chown root:root /etc/sudoers.d/ai
chmod 0440 /etc/sudoers.d/ai
