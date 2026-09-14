#!/bin/bash

mkdir -p image $HOME/.kiwi_boxes

podman run \
    --privileged \
    --pull=newer \
    -v $HOME/.kiwi_boxes:/root/.kiwi_boxes \
    -v $PWD:/podman/aws.kiwi \
    -v $(dirname $PWD)/.sle16:/.sle16 \
    -v $PWD/image:/aws.oci \
    --rm \
    -it public.ecr.aws/b9k1j9y6/kiwi:latest \
    system boxbuild \
    --box tumbleweed \
    --shared-path /.sle16 \
    --box-smp-cpus 2 \
    --box-memory 2048 \
    kiwi \
    --description /podman/aws.kiwi \
    --target-dir /aws.oci
