#!/bin/bash

mkdir -p image $HOME/.kiwi_boxes

podman run \
    --privileged \
    --pull=newer \
    -v $HOME/.kiwi_boxes:/root/.kiwi_boxes \
    -v $PWD:/claude.kiwi \
    -v $PWD/image:/claude.oci \
    --rm \
    -it public.ecr.aws/b9k1j9y6/kiwi:latest \
    system boxbuild \
    --box tumbleweed \
    --box-smp-cpus 2 \
    --box-memory 2048 \
    kiwi \
    --description /claude.kiwi \
    --target-dir /claude.oci
