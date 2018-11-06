#!/bin/bash

source $HOME/.cargo/env
mkdir /foo
cd foo
cp /mnt/Cargo.* .
cp -r /mnt/src .
cargo build --release
mv target/release/fulliautomatisk /mnt/
