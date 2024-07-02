to build run 
```
RUSTFLAGS="-Zlocation-detail=none -C target-cpu=cortex-a53" cargo +nightly zigbuild -Z build-std=std,panic_abort --target aarch64-unknown-linux-gnu.2.31 --release
```
and then
```
upx --best --lzma target/aarch64-unknown-linux-gnu/release/go-upload-server
```
Make sure it is compiled using glibc 2.31 or lower