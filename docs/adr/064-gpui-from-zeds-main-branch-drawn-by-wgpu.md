# ADR-064: gpui from Zed's main branch, drawn by wgpu

Status: accepted, 2026-10-17. Revises ADR-001's pin (gpui 0.2.2 from
crates.io). Amends its known gap: gpui now has AccessKit, and Skwir's
elements are yet to use it.

## Context

- **Skwir drew with gpui 0.2.2, vendored with 24 patches.** On Linux that gpui
  draws with Blade, on Vulkan alone. It has no AccessKit (ADR-001). 0.2.2
  (October 2025) is still the latest release on crates.io.
- **A 3D model preview** that moves and zooms needs a GPU pass of Skwir's own
  in the window, on every system. Today `skwir-model-worker` rasterizes
  stills on the CPU. Zed's main branch draws with wgpu on Linux and the web,
  Metal on macOS and Direct3D 11 on Windows, and integrates AccessKit.
- **Zed's main branch splits gpui into crates.** `gpui`, `gpui_platform` (which
  picks a system's backend), `gpui_linux`, `gpui_wgpu`, `gpui_windows`,
  `gpui_macos` and `gpui_apple`, `gpui_web`, and some twenty support crates.
  None is published.
- **Zed pins its dependencies at commits:**
  - wgpu at a trunk commit, for a fix 30.0.1 lacks: dropping a surface after
    `present` failed, as after a lost device, panicked (gfx-rs/wgpu#10230).
    gpui's recovery from a lost device drops its surface.
  - Its own forks of font-kit, xim and scap.
  - Through its root manifest's `[patch.crates-io]`, crates gpui reaches:
    - khronos-egl: an EGL call that fails with no error reported panicked
      in the unmaintained 6.0. This is reached on Xvfb under NVIDIA's EGL
      whenever GL is started.
    - async-task: a waker's identity.
    - calloop: the event loop's idle work bounded.
    - async-process.
- **`deny.toml` denied every git source.**
- **The spike** (2026-10-17, worktree `spike/gpui-main`) first rewrote the
  pins to crates.io releases: wgpu 30.0.1, three API adaptations, the forks'
  published versions. It built, but without the fixes above. Each update
  would have been that port again.

## Decision

1. **gpui is vendored from a commit of Zed's main branch** in `vendor/zed`
   (`UPSTREAM-COMMIT`), with Skwir's patches.
   - Only the crates `gpui` and `gpui_platform` reach are taken, by path,
     with a workspace manifest trimmed to them.
   - Each patch is a diff in `vendor/patches` in Zed's layout.
   - `tools/gpui/vendor.py` makes the tree from a Zed checkout and the
     patches, and writes them back after an edit.
   - Skwir moves to a newer commit deliberately, as it moved between
     releases before.
2. **Skwir follows Zed's pins**, including the git ones:
   - The vendored manifest keeps them as Zed wrote them.
   - Skwir's root manifest repeats Zed's `[patch.crates-io]` entries for
     the crates gpui reaches, but not notify's, which only Skwir's own
     watcher uses.
   - `deny.toml` allows exactly those repositories, each by a full commit
     (`required-git-spec = "rev"`). One more git source is a decision, named
     there.
3. **On Linux, Vulkan alone where Vulkan offers a GPU that can draw to the
   window; GL beside it otherwise** (patch 0025).
   - Starting GL loads its driver too (NVIDIA's EGL and GL core: 16 MiB, and
     time before the first frame).
   - A machine whose GPU only GL drives (an old one, a virtual machine's)
     still draws on its GPU, and one with no Vulkan driver draws through GL.
   - macOS draws with Metal and Windows with Direct3D 11, as upstream does.
4. **A file drag keeps Skwir's icon past the window's edge** (patch 0001).
   - Upstream hands the drag to the compositor as it leaves the window, on
     Wayland and macOS, with no image of its own.
   - On Wayland the compositor shows Skwir's thumbnail and count, which the
     window also draws under the pointer before the edge.

## Consequences

- **Accessibility can be built.** The window has AccessKit
  (`gpui_platform::application`), but Skwir's elements declare no roles or
  labels yet, so a screen reader reads little. That is the next step for
  ADR-001's gap.
- **The 3D preview** can be a wgpu pass on Linux, with Metal and Direct3D 11
  passes for macOS and Windows.
- **Building needs the network for git sources.** cargo fetches them like
  crates (`cargo fetch` in the AUR packages, the build roots, which now share
  `~/.cargo/git`). The Nix package fetches them at the lock file's commits
  (`allowBuiltinFetchGit`).
- **Patches gone:** 0002 to 0005, 0007, 0009, 0012, 0015, 0016 and 0018. Zed's
  main branch has its own answer to each (`vendor/zed/README.skwir.md`).
  - 0003's invariant changes reason (`docs/invariants/media.md`): wgpu keeps a
    texture until the GPU is done with it.
  - 0014, a few milliseconds of the first frame, is not ported.
- **What stays as it was:**
  - X11 and Windows hand no drag out of the window.
  - X11 polls at the display's rate.
  - Skwir keeps the pointer while keys move through files
    (`CursorHideMode::Never`), where upstream hides it at every bound key.
- **Checks:** `just gpui-check` replaces `list-check` and `atlas-check`; the
  wgpu atlas's own tests cover what 0003's did.
- **macOS remains compiled only for Skwir's other crates.** `check-macos`
  leaves out skwir-ui and skwir, as before: gpui's Metal backend needs Apple's
  SDK.

## Measurements

Reference machine (ADR-000), Hyprland on its NVIDIA GPU. Release builds of
main (4e846d7, gpui 0.2.2) and of this change, measured with
`tools/measure/idle-pss.py`: three starts each, alternated over three rounds
on 2026-10-18. The table gives the medians.

| PSS, MiB                  | gpui 0.2.2 | Zed's gpui |
| ------------------------- | ---------: | ---------: |
| anonymous (heap, stacks)  |       25.5 |       28.5 |
| Skwir's binary            |       41.9 |       47.2 |
| GPU buffers               |       19.5 |       11.6 |
| other files and libraries |        6.6 |        6.7 |
| **own share**             |   **93.4** |   **94.1** |
| GPU driver code           |       33.1 |       43.6 |
| total                     |      126.6 |      137.7 |

- **The own share** (ADR-033's budget) grows 0.7 MiB and stays within 100 MiB.
  wgpu holds 8 MiB fewer GPU buffers than Blade did, while the heap and the
  code grow.
- **NVIDIA's driver** maps 10.5 MiB more of its code for wgpu than for Blade.
  That is outside the budget, and every GPU client on the machine shares
  those pages.
- **The first frame** could not be compared on 2026-10-18. Other programs
  kept the load average between 4 and 17, and starts of the two builds
  ranged from 136 to 802 ms. On 2026-10-17, quiet and alternated, the medians
  were about 118 ms on 0.2.2 and 126 ms on Zed's gpui, then built with wgpu
  30.0.1. The 150 ms budget is to be checked again on a quiet machine.
- **Without a GPU** (lavapipe on a headless sway, 2026-10-17), the own share
  went from 91.2 to 106.7 MiB: lavapipe's swapchain images are Skwir's own
  memory there, and wgpu keeps three. The first frame went 8 ms later.
  0013 halved the pipeline phase (66 to 35 ms).
- **The stripped binary** went from 67.7 to 75.7 MB.
