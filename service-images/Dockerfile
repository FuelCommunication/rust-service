FROM rust:1.93-slim AS builder
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    musl-tools \
    cmake \
    pkg-config \
    build-essential \
    clang \
    libclang-dev \
    libcurl4-openssl-dev \
    && rustup target add x86_64-unknown-linux-musl \
    && ln -s /usr/bin/g++ /usr/bin/musl-g++ \
    && ln -s /usr/include/x86_64-linux-gnu/curl /usr/include/x86_64-linux-musl/curl
WORKDIR /usr/src/app
COPY . ./

ENV CC_x86_64_unknown_linux_musl="musl-gcc"
ENV CXX_x86_64_unknown_linux_musl="musl-g++"

RUN cargo build -p service-images --target x86_64-unknown-linux-musl --release

FROM scratch
COPY --from=builder /usr/src/app/target/x86_64-unknown-linux-musl/release/service-images /usr/local/bin/service-images
USER 1000
WORKDIR /app
EXPOSE 3000
ENTRYPOINT ["/usr/local/bin/service-images"]
