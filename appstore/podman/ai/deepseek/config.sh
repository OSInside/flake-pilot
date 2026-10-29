#!/bin/bash

set -ex

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
pi install npm:pi-permission-modes
pi install npm:@narumitw/pi-plan-mode
pi install npm:@juicesharp/rpiv-ask-user-question
pi install npm:@juicesharp/rpiv-todo
pi install npm:pi-lens
pi install npm:@dietrichgebert/ponytail
pi install npm:pi-web-access

# Gemini model providers
git clone -b changelog-unreleased https://github.com/openSUSE-Python/pi-gemini-acp \
  ~/checkout/pi-gemini-acp
pi install ~/checkout/pi-gemini-acp

# Agy model providers
pi install npm:@estebanforge/pi-antigravity-bridge
)

chown -R ai:users /home/ai
