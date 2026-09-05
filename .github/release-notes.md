Prebuilt binaries — no Rust toolchain required.

**macOS (Apple silicon)**
```sh
curl -sSfL https://github.com/__REPO__/releases/download/__TAG__/pf-aarch64-apple-darwin.tar.gz | tar xz
xattr -d com.apple.quarantine pf 2>/dev/null || true   # unsigned build
sudo mv pf /usr/local/bin/
```

**macOS (Intel)** — same, with `pf-x86_64-apple-darwin.tar.gz`.

**Linux (x86_64)**
```sh
curl -sSfL https://github.com/__REPO__/releases/download/__TAG__/pf-x86_64-unknown-linux-gnu.tar.gz | tar xz
sudo mv pf /usr/local/bin/
```

Checksums are in `SHA256SUMS`. Then run `pf auth login`.
