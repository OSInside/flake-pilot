#!/bin/bash

mkdir -p image

podman run \
    --privileged \
    --pull=newer \
    -v $HOME/.kiwi_boxes:/root/.kiwi_boxes \
    -v $PWD:/fedora.kiwi \
    -v $PWD/image:/fedora.kis \
    --rm \
    -it public.ecr.aws/b9k1j9y6/kiwi:latest \
    system boxbuild \
    --box universal \
    kiwi \
    --description /fedora.kiwi \
    --target-dir /fedora.kis
