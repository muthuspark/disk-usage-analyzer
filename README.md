# Disk Usage Analyzer

A native Rust desktop app built with GPUI. Shows disk usage as a folder tree,
rings chart, or treemap. Every launch performs a fresh scan with up to eight
workers; scan results are never saved.

Requires Rust, Linux with Vulkan, and development libraries for FreeType,
fontconfig, Wayland, xkbcommon, and xkbcommon-x11.

```bash
git clone https://github.com/muthuspark/disk-usage-analyzer.git
cd disk-usage-analyzer
./scripts/run.sh
```

Scans `/` by default. To scan your home folder:

```bash
./scripts/run.sh -- "$HOME"
```

Click folders or chart segments to drill down. Use the back arrow or breadcrumbs
to navigate, the bottom buttons to switch charts, and refresh to rescan.

Sizes measure allocated disk space on the selected filesystem. Symlinks are not
followed, and unreadable directories are reported. For a release build, run
`./scripts/run.sh --release`.
