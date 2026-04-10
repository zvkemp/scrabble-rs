FROM rust:1.91.1-slim-bullseye AS builder
WORKDIR /app

RUN apt-get update && apt-get install curl -y
RUN curl https://get.volta.sh | bash

RUN export PATH=/root/.volta/bin:$PATH && volta install node@24.11.1 && volta install yarn@4.12.0

COPY Cargo.toml Cargo.lock ./
RUN mkdir src/ && echo "fn main() {}" > src/main.rs && cargo build --release
RUN rm -rf src/main.rs target/release/deps/scrabble-*
COPY . .

ENV PATH="$PATH:/root/.volta/bin"
ENV SQLX_OFFLINE=true
RUN echo $PATH
RUN ls -al /root/.volta/bin/
RUN yarn install

RUN cargo build --release

FROM debian:bullseye-slim
WORKDIR /usr/local/bin
COPY --from=builder /app/target/release/scrabble .
CMD ["./scrabble"]
