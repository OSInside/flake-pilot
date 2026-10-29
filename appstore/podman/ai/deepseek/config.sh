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

mkdir /home/ai/shared

export HOME=/home/ai
export PATH=$HOME/.local/bin:$PATH

curl -fsSL https://pi.dev/install.sh | sh
curl -fsSL https://pi-bolt.opensec.in/install.sh | sh
curl -fsSL https://antigravity.google/cli/install.sh | bash

pi install npm:@raidou/pi-notify
pi install npm:pi-open-tui
pi install npm:@inobit/pi-themes
pi install npm:@twogiants/pi-anthropic-vertex
pi install npm:@monotykamary/pi-retry
pi install npm:pi-llama-chooser
# pi install npm:pi-permission-modes
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

# deepseek models
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
)

chown -R ai:users /home/ai
