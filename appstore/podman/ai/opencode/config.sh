#!/bin/bash
set -ex

mkdir -p /root/ai

# shellcheck disable=SC2016
echo 'export PATH=/root/.opencode/bin:$PATH' > /root/.bashrc

export HOME=~

curl -fsSL https://opencode.ai/install > opencode.install.sh
bash opencode.install.sh

test -d /root/.opencode
