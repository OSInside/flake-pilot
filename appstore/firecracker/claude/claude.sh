#!/bin/bash

mkdir -p image

podman run \
	--privileged \
	-v $HOME/.kiwi_boxes:/root/.kiwi_boxes \
	-v $PWD:/claude.kiwi \
	-v $PWD/image:/claude.kis \
	--rm \
	-it public.ecr.aws/b9k1j9y6/kiwi:latest \
	system boxbuild \
	--box tumbleweed \
	-- \
	--description /claude.kiwi \
	--target-dir /claude.kis
