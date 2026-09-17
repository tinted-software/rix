# Rix

A cross-platform Nix implementation in Rust.

## Overview

This project is based on:

- [Tom's nix-eval crate](https://github.com/Industrial/nixos-dotfiles/tree/main/rust/tools/nix-eval)
- [rnix parser](https://github.com/nix-community/rnix-parser)

## Cross compilation (work in progress)

Rix tracks two platform triples (a `cpu-os` pair such as `x86_64-linux` or
`x86_64-darwin`) while building:

- the *build* platform performs the build (the machine running `rix`), and
- the *host* platform runs the produced package.

They are configured with global flags:

```console
$ rix build -A xnu --system x86_64-linux --cross-system x86_64-darwin
```

- `--system` sets the build platform and what `builtins.currentSystem`
  reports. It defaults to the running machine.
- `--cross-system` sets the host platform. When it differs from the build
  platform the build is a cross build.
- `--runner` gives the command used to execute host-platform binaries during
  the build (for example `--runner darling` for Darwin binaries on Linux, or
  `--runner wine` for Windows).

The platforms are exposed to build scripts as `$buildPlatform`,
`$hostPlatform` and `$system` (the host platform), and to Nix expressions as

- `builtins.currentSystem` / `builtins.currentBuildPlatform`,
- `builtins.currentHostPlatform`, and
- `builtins.isCrossCompiling`.

When a runner is configured, Rix installs a `rix-run-host` helper on `PATH`
(its path is also exported as `$RIX_TARGET_RUNNER`) that executes a
host-platform binary through the runner:

```sh
rix-run-host ./target-binary --some-flag
```

Without an explicit runner, same-OS foreign-CPU builds (for example
`x86_64-linux` -> `aarch64-linux`) fall back to a `qemu-<arch>-static` style
emulator, and other combinations only warn that host binaries cannot run.
Rix deliberately does not guess a runner from `PATH` so that a misconfigured
cross build fails loudly rather than silently doing the wrong thing.
