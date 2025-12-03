# Seance Packaging

This crate is run by CI to build `seance` and `planchette` for
all target architectures. The result of a run of `packaging` is
a directory called `seance-distribution` that contains:

- The `seance` binary for:
    - `x86_64-linux`
    - `x86_64-windows`
- The `planchette` binary for `armv6l-linux`
- A `.deb` for installing `planchette` on Debian-based
    `armv6l-linux` and `x86_64-linux` systems.
- Suitable packaging for the `seance` binary for:
    - Debian-based distributions
    - Arch-based distributions

NixOS users are expected to use the `flake.nix` provided in the
    root of this repo to include `seance` in their system config.
The packaging for each supported system will place `seance` in a
location that is commonly available in `$PATH` in standard
installations (generally `/usr/bin/`) and will attempt to provide
a `seance.desktop` file that relies on `seance` being in `$PATH`.
If the target system does not have `$XDG_DATA_DIRS` set then the
`.desktop` file will not be emplaced. `$XDG_DATA_DIRS/applications/`
will be created if required unless the guidelines for the target
distribution say not to.

It will make no difference to the result to run `packaging` in
either `debug` or `release`, `packaging` will always build a
release version of each binary it builds.

This binary **requires** `nix` with support for flakes. `packaging`
shells out with reckless abandon, relying on `nix build` to build
for each target architecture.

It is expected that `packaging` will be run from the root of this
repository via e.g. `cargo run --release --bin packaging`. Ideally
this entire binary would be a `cargo-script` but the `cargo-script`
feature is not yet stabilized¹². Until this feature is stabilized
we require that `packaging` be run from the workspace root in order
to avoid current working directory shenanigans.

---

¹ https://github.com/rust-lang/rust-project-goals/issues/119
² https://github.com/rust-lang/cargo/issues/12207
