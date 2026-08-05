// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! OAM / VRAM / sprite / mode-2-3 pinned-behavior tests.
//!
//! Split into cohesive submodules to stay under the 1000-line cap:
//! [`access`] holds the OAM/VRAM access + mode-2/3 STAT-read verdict tests,
//! [`render`] holds the mid-mode-3 pixel-render (`*_m3_render_*`) tests. All
//! shared helpers live in the parent `gambatte` module (`use super::super::*`).

#[path = "oam_vram/access.rs"]
mod access;
#[path = "oam_vram/render.rs"]
mod render;
