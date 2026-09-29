#!/bin/bash

mkdir -p image

podman run \
    --privileged \
    --pull=newer \
    -v $HOME/.kiwi_boxes:/root/.kiwi_boxes \
    -v $PWD:/deepseek.kiwi \
    -v $PWD/image:/deepseek.kis \
    --rm \
    -it public.ecr.aws/b9k1j9y6/kiwi:latest \
    system boxbuild \
    --box tumbleweed \
    kiwi \
    --description /deepseek.kiwi \
    --target-dir /deepseek.kis
