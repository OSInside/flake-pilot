#!/bin/bash

mkdir -p image $HOME/.kiwi_boxes

podman run \
    --privileged \
    --pull=newer \
    -v $HOME/.kiwi_boxes:/root/.kiwi_boxes \
    -v $PWD:/opencode.kiwi \
    -v $PWD/image:/opencode.oci \
    --rm \
    -it public.ecr.aws/b9k1j9y6/kiwi:latest \
    system boxbuild \
    --box tumbleweed \
    --box-smp-cpus 2 \
    --box-memory 2048 \
    kiwi \
    --description /opencode.kiwi \
    --target-dir /opencode.oci
