FROM ubuntu:18.04

RUN apt-get update
RUN apt-get install -y libssl-dev pkg-config vim git curl build-essential
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y
RUN echo 'source $HOME/.cargo/env' >> $HOME/.bashrc
