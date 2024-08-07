to build run 
```
RUSTFLAGS="-Zlocation-detail=none -C target-cpu=cortex-a53" cargo +nightly zigbuild -Z build-std=std,panic_abort --target aarch64-unknown-linux-gnu.2.31 --release
```
and then
```
upx --best --lzma target/aarch64-unknown-linux-gnu/release/go-upload-server
```
Make sure it is compiled using glibc 2.31 or lower

package it as .deb:
```
cargo deb --no-build --target aarch64-unknown-linux-gnu --no-strip
```

sign the package:
```
dpkg-sig --sign builder target/aarch64-unknown-linux-gnu/debian/go-upload-server_*_arm64.deb
```