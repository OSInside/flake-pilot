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

zypper ar https://download.opensuse.org/distribution/leap/16.0/repo/oss Leap

npm install -g @earendil-works/pi-coding-agent@latest

mkdir -p /home/ai/.pi/agent
cat > /home/ai/.pi/agent/models.json <<'EOF'
{
  "providers": {
    "deepseek": {
      "baseUrl": "https://api.deepseek.com",
      "api": "openai-completions",
      "apiKey": "$DEEPSEEK_API_KEY",
      "models": [
        {
          "id": "deepseek-v4-pro",
          "name": "DeepSeek V4 Pro",
          "contextWindow": 1000000,
          "maxTokens": 384000,
          "input": ["text"],
          "reasoning": true,
          "cost": {
            "input": 1.74,
            "output": 3.48,
            "cacheRead": 0.145,
            "cacheWrite": 0
          },
          "compat": {
            "requiresReasoningContentOnAssistantMessages": true,
            "thinkingFormat": "deepseek",
            "reasoningEffortMap": {
              "minimal": "high",
              "low": "high",
              "medium": "high",
              "high": "high",
              "xhigh": "max"
            }
          }
        },
        {
          "id": "deepseek-v4-flash",
          "name": "DeepSeek V4 Flash",
          "contextWindow": 1000000,
          "maxTokens": 384000,
          "input": ["text"],
          "reasoning": true,
          "cost": {
            "input": 0.14,
            "output": 0.28,
            "cacheRead": 0.028,
            "cacheWrite": 0
          },
          "compat": {
            "requiresReasoningContentOnAssistantMessages": true,
            "thinkingFormat": "deepseek",
            "reasoningEffortMap": {
              "minimal": "high",
              "low": "high",
              "medium": "high",
              "high": "high",
              "xhigh": "max"
            }
          }
        }
      ]
    }
  }
}
EOF

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
