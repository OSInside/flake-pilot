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

zypper ar https://download.opensuse.org/distribution/leap/16.0/repo/oss Leap

npm install -g @anthropic-ai/claude-code@latest

curl https://sdk.cloud.google.com > install.sh
bash install.sh --disable-prompts --install-dir=/usr/share

ln -s /usr/share/google-cloud-sdk/bin/gcloud /usr/bin/gcloud
ln -s /usr/share/google-cloud-sdk/bin/gsutil /usr/bin/gsutil
ln -s /usr/share/google-cloud-sdk/bin/bq /usr/bin/bq

mkdir -p /etc/claude-code

cat > /etc/claude-code/managed-settings.json <<'EOF'
{
  "permissions": {
    "defaultMode": "default"
  },
  "env": {
    "CLAUDE_CODE_USE_VERTEX": "1",
    "CLOUD_ML_REGION": "global",
    "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-5",
    "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-opus-5",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-haiku-4-5@20251001"
  }
}
EOF
chmod 0644 /etc/claude-code/managed-settings.json


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
