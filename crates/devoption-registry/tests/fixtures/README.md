# Trusted offline build fixtures

`offline-app` is a tiny DevOption-authored program with one binary. It prints a
value formatted by the sole enabled dependency, `itoa = "=1.0.18"`. Neither the
app nor that dependency has a build script. The selected feature set has no
transitive dependencies; upstream's optional `no-panic` dependency and development
dependencies are not enabled or built.

`registry/itoa-1.0.18.crate` is the unmodified public crates.io archive already
pinned by DevOption's workspace lockfile. It was copied from the existing public
Cargo cache, not from a private prototype or registered repository. Its 15,935
bytes have SHA-256:

```text
8f42a60cbdf9a97f5d2305f08a87dc4e09308d1276d28c869c684d7777685682
```

Upstream: https://crates.io/crates/itoa/1.0.18
Source: https://github.com/dtolnay/itoa
Copyright/license: David Tolnay and contributors, MIT OR Apache-2.0. The original
license texts and attribution remain inside the unchanged archive.

`registry/itoa.index` is the corresponding version record from the public registry
index. It retains upstream dependency metadata and checksum; it does not invent
a private package in the crates.io namespace. The test writes that record and
provider-supplied archive to Cargo's documented local-registry layout.

Only the Linux integration test compiles/runs this authored app, in fresh temporary
directories. No fixture is imported into the running service or real catalog.
