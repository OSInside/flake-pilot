FROM debian:trixie
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates build-essential pkg-config make git rustup \
    libssl-dev python3-docutils python3-pygments podman bubblewrap \
    sudo
WORKDIR /src
RUN git clone https://github.com/OSInside/flake-pilot.git .
RUN rustup default stable
RUN make build
RUN cargo test
RUN make install
RUN cp flakes.yml /etc
