# Trusted public WASIp2 closure

These are unmodified public crates.io archives, copied from the public Cargo
cache and verified against `examples/static-site/Cargo.lock`. SHA-256 pins are
also enforced independently in `src/library/vendor/wasip2.rs`:

| Archive | SHA-256 |
| --- | --- |
| wasip2-1.0.1+wasi-0.2.4.crate | 0562428422c63773dad2c345a1882263bbf4d65cf3f42e90921f787ef5ad58e7 |
| wit-bindgen-0.46.0.crate | f17a85883d4e6d00e8a97c586de764dabcc06133f7f1d55dce5cdc070ad7fe59 |
| bitflags-2.13.2.crate | 3ded4057c258ba199e2d26386d3af3780957ecaee6c4ef4041c6b4b8b97c0b06 |

Upstream sources: <https://github.com/bytecodealliance/wasi-rs>,
<https://github.com/bytecodealliance/wit-bindgen>,
<https://github.com/bitflags/bitflags>.
Original license texts and attribution are retained in each archive. wasip2 and
wit-bindgen use Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT; bitflags uses
MIT OR Apache-2.0. No private source or configured installation is included.

Ordinary Rust tests exercise the actual archives, manifest refusal under the
restricted general profile, pinned-policy acceptance and mutation/graph/namespace
refusal. The disposable Linux installed acceptance uses them as private operator
inputs, without downloads in the worker or dependency acquisition fallback.
