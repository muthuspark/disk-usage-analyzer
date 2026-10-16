mod chart;
mod model;
mod scan;
mod theme;

use gpui::{prelude::*, *};
use model::Node;
use std::{
    cell::RefCell,
    collections::HashSet,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, atomic::Ordering, mpsc},
    time::Duration,
};

const BODY_SIZE: f32 = 13.;
const SIZE_COLUMN: f32 = 83.;
const CONTENTS_COLUMN: f32 = 120.;
const MODIFIED_COLUMN: f32 = 80.;

struct Analyzer {
    font_family: SharedString,
    root: Node,
    path: Vec<usize>,
    expanded: HashSet<Vec<usize>>,
    descending: bool,
    treemap: bool,
    hovered: Option<(String, String)>,
    chart_drawing: Rc<RefCell<chart::Drawing>>,
    list_scroll: UniformListScrollHandle,
    list_path: Vec<usize>,
    scan_root: PathBuf,
    progress: Arc<scan::Progress>,
    scanning: bool,
    status: String,
}

impl Analyzer {
    fn start_scan(&mut self, cx: &mut Context<Self>) {
        self.progress.cancel.store(true, Ordering::Relaxed);
        let progress = Arc::new(scan::Progress::default());
        self.progress = progress.clone();
        self.scanning = true;
        self.status = format!("Scanning {}…", self.scan_root.display());
        self.path.clear();
        self.list_scroll
            .scroll_to_item_strict(0, ScrollStrategy::Top);
        self.expanded.clear();
        self.hovered = None;
        let label = self.root.name.clone();
        self.root = Node::empty(label.clone());
        let path = self.scan_root.clone();
        let (sender, receiver) = mpsc::channel();
        let worker_progress = progress.clone();
        std::thread::spawn(move || {
            let _ = sender.send(scan::scan(&path, label, &worker_progress));
        });
        cx.spawn(async move |this, cx| {
            loop {
                Timer::after(Duration::from_millis(200)).await;
                if progress.cancel.load(Ordering::Relaxed) { break; }
                let done = this.update(cx, |this, cx| {
                    let entries = progress.entries.load(Ordering::Relaxed);
                    let errors = progress.errors.load(Ordering::Relaxed);
                    let mounts = progress.skipped_mounts.load(Ordering::Relaxed);
                    let done = match receiver.try_recv() {
                        Ok(Ok(report)) => {
                            this.root = report.root;
                            this.scanning = false;
                            this.status = format!("{} · {entries} entries · {:.1}s · {errors} unreadable · {mounts} other mounts skipped", this.scan_root.display(), report.seconds);
                            true
                        }
                        Ok(Err(error)) => {
                            this.scanning = false;
                            this.status = format!("Could not scan {}: {error}", this.scan_root.display());
                            true
                        }
                        Err(mpsc::TryRecvError::Empty) => {
                            this.status = format!("Scanning {}… {entries} entries · {errors} unreadable", this.scan_root.display());
                            false
                        }
                        Err(mpsc::TryRecvError::Disconnected) => {
                            this.scanning = false;
                            this.status = "Scan stopped unexpectedly. Click refresh to retry.".into();
                            true
                        }
                    };
                    cx.notify();
                    done
                }).unwrap_or(true);
                if done { break; }
            }
        }).detach();
        cx.notify();
    }

    fn current(&self) -> &Node {
        self.root.at(&self.path)
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        self.path.pop();
        self.hovered = None;
        cx.notify();
    }

    fn rows(&self, path: Vec<usize>, depth: usize, rows: &mut Vec<(Vec<usize>, usize, Node)>) {
        let node = self.root.at(&path);
        let mut indices: Vec<_> = (0..node.children.len()).collect();
        indices.sort_by(|a, b| {
            let order = node.children[*a].bytes.cmp(&node.children[*b].bytes);
            if self.descending {
                order.reverse()
            } else {
                order
            }
        });
        for i in indices {
            let mut child_path = path.clone();
            child_path.push(i);
            rows.push((child_path.clone(), depth, node.children[i].clone()));
            if self.expanded.contains(&child_path) {
                self.rows(child_path, depth + 1, rows);
            }
        }
    }

    fn row(
        &self,
        node: Node,
        path: Vec<usize>,
        depth: usize,
        root: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let pending = root && self.scanning;
        let expandable = !node.children.is_empty();
        let expanded = self.expanded.contains(&path);
        let toggle_path = path.clone();
        let total = self.current().bytes;
        let percent = if total > 0 {
            node.bytes as f64 / total as f64 * 100.
        } else {
            0.
        };
        div()
            .id(SharedString::from(format!("row-{path:?}-{root}")))
            .w_full()
            .flex()
            .items_center()
            .h(px(28.))
            .text_size(px(BODY_SIZE))
            .whitespace_nowrap()
            .cursor_pointer()
            .hover(|s| s.bg(rgb(theme::SURFACE)))
            .on_click(cx.listener(move |this, _, _, cx| {
                if expandable && !root {
                    this.path = path.clone();
                    this.hovered = None;
                    cx.notify();
                }
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .pl(px(if root { 4. } else { 4. + depth as f32 * 14. }))
                    .when(!root, |s| {
                        s.child(
                            div()
                                .id(SharedString::from(format!("expand-{toggle_path:?}")))
                                .w(px(18.))
                                .flex_shrink_0()
                                .text_color(rgb(theme::EMPHASIS))
                                .child(if expandable {
                                    if expanded { "▾" } else { "▸" }
                                } else {
                                    ""
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    if !this.expanded.remove(&toggle_path) {
                                        this.expanded.insert(toggle_path.clone());
                                    }
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .w(px(39.))
                                .flex_shrink_0()
                                .mr(px(6.))
                                .bg(rgb(theme::SURFACE))
                                .relative()
                                .child(
                                    div()
                                        .absolute()
                                        .h_full()
                                        .w(px(39. * percent as f32 / 100.))
                                        .bg(rgba((theme::BLUE << 8) | 0x38)),
                                )
                                .child(
                                    div()
                                        .relative()
                                        .text_right()
                                        .pr(px(2.))
                                        .child(format!("{}%", percent.floor() as u32)),
                                ),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .line_clamp(1)
                            .whitespace_normal()
                            .text_ellipsis()
                            .text_color(rgb(if node.restricted {
                                theme::RED
                            } else {
                                theme::EMPHASIS
                            }))
                            .font_weight(if root {
                                FontWeight::BOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .child(node.name),
                    ),
            )
            .child(
                div()
                    .w(px(SIZE_COLUMN))
                    .flex_shrink_0()
                    .text_right()
                    .pr(px(8.))
                    .child(if pending {
                        "…".into()
                    } else if node.restricted {
                        "—".into()
                    } else {
                        model::format_size(node.bytes)
                    }),
            )
            .child(
                div()
                    .w(px(CONTENTS_COLUMN))
                    .flex_shrink_0()
                    .text_right()
                    .pr(px(8.))
                    .child(if pending {
                        "…".into()
                    } else if node.restricted {
                        "—".into()
                    } else {
                        format!("{} items", node.items)
                    }),
            )
            .child(
                div()
                    .w(px(MODIFIED_COLUMN))
                    .flex_shrink_0()
                    .pr(px(8.))
                    .text_right()
                    .child(node.modified),
            )
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut breadcrumbs = vec![(format!("▣  {}", self.root.name), vec![])];
        let mut node = &self.root;
        for (depth, &i) in self.path.iter().enumerate() {
            node = &node.children[i];
            breadcrumbs.push((node.name.clone(), self.path[..=depth].to_vec()));
        }
        div()
            .flex()
            .items_center()
            .gap(px(9.))
            .h(px(46.))
            .px(px(10.))
            .flex_shrink_0()
            .bg(rgb(theme::SURFACE))
            .border_b_1()
            .border_color(rgb(theme::BORDER))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
            .child(
                button("back", "‹")
                    .text_size(px(26.))
                    .opacity(if self.path.is_empty() { 0.4 } else { 1. })
                    .on_click(cx.listener(|this, _, _, cx| this.back(cx))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(35.))
                    .flex()
                    .items_center()
                    .rounded(px(5.))
                    .border_1()
                    .border_color(rgb(theme::BORDER))
                    .bg(rgb(theme::BACKGROUND))
                    .child(
                        div()
                            .id("breadcrumbs")
                            .flex()
                            .items_center()
                            .min_w_0()
                            .overflow_x_scroll()
                            .h_full()
                            .children(breadcrumbs.into_iter().enumerate().map(
                                |(depth, (name, path))| {
                                    div()
                                        .flex()
                                        .items_center()
                                        .h_full()
                                        .flex_shrink_0()
                                        .when(depth > 0, |s| {
                                            s.child(
                                                div()
                                                    .px(px(3.))
                                                    .text_color(rgb(theme::EMPHASIS))
                                                    .child("/"),
                                            )
                                        })
                                        .child(
                                            div()
                                                .id(SharedString::from(format!(
                                                    "breadcrumb-{depth}"
                                                )))
                                                .h_full()
                                                .flex()
                                                .items_center()
                                                .px(px(10.))
                                                .bg(rgb(theme::SURFACE))
                                                .font_weight(FontWeight::BOLD)
                                                .cursor_pointer()
                                                .hover(|s| s.bg(rgba((theme::BLUE << 8) | 0x1f)))
                                                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                                    cx.stop_propagation()
                                                })
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.path = path.clone();
                                                    this.hovered = None;
                                                    cx.notify();
                                                }))
                                                .child(name),
                                        )
                                },
                            )),
                    ),
            )
            .child(
                button("reset", "⟳")
                    .text_size(px(23.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.start_scan(cx);
                    })),
            )
            .child(
                button("minimize", "−")
                    .bg(rgb(theme::BACKGROUND))
                    .rounded_full()
                    .on_click(|_, w, _| w.minimize_window()),
            )
            .child(
                button("maximize", "□")
                    .bg(rgb(theme::BACKGROUND))
                    .rounded_full()
                    .on_click(|_, w, _| w.zoom_window()),
            )
            .child(
                button("close", "×")
                    .bg(rgb(theme::BACKGROUND))
                    .rounded_full()
                    .on_click(|_, _, cx| cx.quit()),
            )
    }
}

fn button(id: &'static str, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(26.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .hover(|s| {
            s.bg(rgba((theme::BLUE << 8) | 0x1f))
                .text_color(rgb(theme::EMPHASIS))
        })
        .child(label)
}

fn ui_monospace(cx: &App) -> SharedString {
    let installed = cx.text_system().all_font_names();
    // GPUI expects a concrete family name, rather than CSS's `ui-monospace` keyword.
    #[cfg(target_os = "linux")]
    if let Ok(output) = std::process::Command::new("fc-match")
        .args(["--format=%{family}", "monospace"])
        .output()
        && output.status.success()
    {
        let families = String::from_utf8_lossy(&output.stdout);
        if let Some(family) = families
            .split(',')
            .map(str::trim)
            .find(|family| installed.iter().any(|name| name == family))
        {
            return family.to_owned().into();
        }
    }
    [
        "SF Mono",
        "Menlo",
        "Consolas",
        "DejaVu Sans Mono",
        "Liberation Mono",
        "Noto Sans Mono",
        "Courier New",
    ]
    .into_iter()
    .find(|family| installed.iter().any(|name| name == family))
    .unwrap_or(".SystemUIFont")
    .into()
}

impl Render for Analyzer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.current().clone();
        if self.list_path != self.path {
            self.list_scroll
                .scroll_to_item_strict(0, ScrollStrategy::Top);
            self.list_path = self.path.clone();
        }
        let mut rows = vec![(self.path.clone(), 0, current.clone())];
        self.rows(self.path.clone(), 0, &mut rows);
        let chart_node = current.clone();
        let drawing_for_layout = self.chart_drawing.clone();
        let drawing_for_paint = self.chart_drawing.clone();
        let treemap = self.treemap;
        let path = self.path.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme::BACKGROUND))
            .text_color(rgb(theme::TEXT))
            .text_size(px(BODY_SIZE))
            .font_family(self.font_family.clone())
            .child(self.toolbar(cx))
            .child(
                div()
                    .h(px(24.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .px(px(8.))
                    .text_size(px(11.))
                    .text_color(rgb(theme::EMPHASIS))
                    .bg(rgb(theme::SURFACE))
                    .child(self.status.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .w(relative(0.5))
                            .min_w(px(420.))
                            .flex_shrink_0()
                            .h_full()
                            .flex()
                            .flex_col()
                            .border_r_1()
                            .border_color(rgb(theme::BORDER))
                            .child(
                                div()
                                    .h(px(31.))
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .text_size(px(11.))
                                    .text_color(rgb(theme::EMPHASIS))
                                    .border_b_1()
                                    .border_color(rgb(theme::BORDER))
                                    .child(div().flex_1().pl(px(4.)).child("Folder"))
                                    .child(
                                        div()
                                            .id("sort")
                                            .w(px(SIZE_COLUMN))
                                            .flex_shrink_0()
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_end()
                                            .gap(px(6.))
                                            .pr(px(8.))
                                            .child("Size")
                                            .child(if self.descending { "▾" } else { "▴" })
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.descending = !this.descending;
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        div()
                                            .w(px(CONTENTS_COLUMN))
                                            .flex_shrink_0()
                                            .text_right()
                                            .pr(px(8.))
                                            .child("Contents"),
                                    )
                                    .child(
                                        div()
                                            .w(px(MODIFIED_COLUMN))
                                            .flex_shrink_0()
                                            .text_right()
                                            .pr(px(8.))
                                            .child("Modified"),
                                    ),
                            )
                            .child(
                                uniform_list(
                                    "folder-list",
                                    rows.len(),
                                    cx.processor(
                                        move |this, range: std::ops::Range<usize>, _, cx| {
                                            range
                                                .map(|i| {
                                                    let (path, depth, node) = &rows[i];
                                                    this.row(
                                                        node.clone(),
                                                        path.clone(),
                                                        *depth,
                                                        i == 0,
                                                        cx,
                                                    )
                                                })
                                                .collect::<Vec<_>>()
                                        },
                                    ),
                                )
                                .track_scroll(self.list_scroll.clone())
                                .flex_1()
                                .min_h_0()
                                .w_full(),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .bg(rgb(theme::BACKGROUND))
                            .child(
                                div()
                                    .id("chart")
                                    .bg(rgb(theme::CHART_BACKGROUND))
                                    .text_color(rgb(theme::TEXT))
                                    .flex_1()
                                    .min_h_0()
                                    .relative()
                                    .child(
                                        canvas(
                                            move |bounds, _, _| {
                                                drawing_for_layout.borrow_mut().update(
                                                    &chart_node,
                                                    bounds,
                                                    treemap,
                                                );
                                            },
                                            move |_, _, window, _| {
                                                drawing_for_paint.borrow().paint(window);
                                            },
                                        )
                                        .size_full(),
                                    )
                                    .when(!treemap, |s| {
                                        s.child(
                                            div()
                                                .absolute()
                                                .top_0()
                                                .left_0()
                                                .size_full()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .child(
                                                    div()
                                                        .id("chart-center")
                                                        .size(px(70.))
                                                        .rounded_full()
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .text_size(px(12.))
                                                        .cursor_pointer()
                                                        .child(if self.scanning {
                                                            "Scanning…".into()
                                                        } else {
                                                            model::format_size(current.bytes)
                                                        })
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.back(cx)
                                                        })),
                                                ),
                                        )
                                    })
                                    .on_mouse_move(cx.listener(
                                        move |this, event: &MouseMoveEvent, _, cx| {
                                            let hit = this
                                                .chart_drawing
                                                .borrow()
                                                .hit(event.position)
                                                .map(|p| {
                                                    let n = this.current().at(p);
                                                    (n.name.clone(), model::format_size(n.bytes))
                                                });
                                            if this.hovered != hit {
                                                this.hovered = hit;
                                                cx.notify();
                                            }
                                        },
                                    ))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                            let hit = this
                                                .chart_drawing
                                                .borrow()
                                                .hit(event.position)
                                                .map(<[usize]>::to_vec);
                                            if let Some(p) = hit
                                                && !this.current().at(&p).children.is_empty()
                                            {
                                                this.path = [path.clone(), p].concat();
                                                this.hovered = None;
                                                cx.notify();
                                            }
                                        }),
                                    ),
                            )
                            .child(
                                div()
                                    .h(px(46.))
                                    .flex_shrink_0()
                                    .relative()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(rgb(theme::SURFACE))
                                    .child(
                                        div()
                                            .absolute()
                                            .left(px(10.))
                                            .text_size(px(11.))
                                            .text_color(rgb(theme::EMPHASIS))
                                            .child(
                                                self.hovered
                                                    .as_ref()
                                                    .map(|(name, size)| format!("{name} · {size}"))
                                                    .unwrap_or_default(),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .rounded(px(7.))
                                            .overflow_hidden()
                                            .bg(rgb(theme::SURFACE))
                                            .child(
                                                button("rings", "◉")
                                                    .size(px(34.))
                                                    .when(!treemap, |s| {
                                                        s.bg(rgb(theme::BLUE))
                                                            .text_color(rgb(theme::BACKGROUND))
                                                    })
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.treemap = false;
                                                        this.hovered = None;
                                                        cx.notify();
                                                    })),
                                            )
                                            .child(
                                                button("treemap", "▥")
                                                    .size(px(34.))
                                                    .when(treemap, |s| {
                                                        s.bg(rgb(theme::BLUE))
                                                            .text_color(rgb(theme::BACKGROUND))
                                                    })
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.treemap = true;
                                                        this.hovered = None;
                                                        cx.notify();
                                                    })),
                                            ),
                                    ),
                            ),
                    ),
            )
    }
}

impl Drop for Analyzer {
    fn drop(&mut self) {
        self.progress.cancel.store(true, Ordering::Relaxed);
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let first = args.next();
    if matches!(first.as_deref(), Some("--help" | "-h")) {
        println!(
            "Usage: disk-usage-analyzer [DIRECTORY]\n       disk-usage-analyzer --scan-only [DIRECTORY]\nDefault: / (current filesystem; symlinks are not followed)"
        );
        return;
    }
    let scan_only = first.as_deref() == Some("--scan-only");
    let scan_root =
        PathBuf::from(if scan_only { args.next() } else { first }.unwrap_or_else(|| "/".into()));
    let scan_root = std::fs::canonicalize(&scan_root).unwrap_or(scan_root);
    let label = if scan_root == std::path::Path::new("/") {
        std::env::var_os("HOME")
            .and_then(|home| {
                PathBuf::from(home)
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "/".into())
    } else {
        scan_root
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    };
    if scan_only {
        let progress = scan::Progress::default();
        match scan::scan(&scan_root, label, &progress) {
            Ok(report) => {
                println!(
                    "{}\t{}\t{}",
                    report.root.bytes,
                    report.root.items,
                    scan_root.display()
                );
                for node in report.root.children.iter() {
                    println!("{}\t{}\t{}", node.bytes, node.items, node.name);
                }
                eprintln!(
                    "{:.2}s; {} unreadable; {} other mounts skipped",
                    report.seconds,
                    progress.errors.load(Ordering::Relaxed),
                    progress.skipped_mounts.load(Ordering::Relaxed)
                );
            }
            Err(error) => {
                eprintln!("Scan failed: {error}");
                std::process::exit(1);
            }
        }
        return;
    }
    Application::new().run(move |cx| {
        let font_family = ui_monospace(cx);
        let bounds = Bounds::centered(None, size(px(906.), px(698.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: None,
                window_min_size: Some(size(px(760.), px(520.))),
                app_id: Some("gpui-disk-analyzer".into()),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|cx| {
                    let mut analyzer = Analyzer {
                        font_family,
                        root: Node::empty(label),
                        path: vec![],
                        expanded: HashSet::new(),
                        descending: true,
                        treemap: false,
                        hovered: None,
                        chart_drawing: Rc::new(RefCell::new(chart::Drawing::default())),
                        list_scroll: UniformListScrollHandle::new(),
                        list_path: vec![],
                        scan_root,
                        progress: Arc::new(scan::Progress::default()),
                        scanning: false,
                        status: String::new(),
                    };
                    analyzer.start_scan(cx);
                    analyzer
                })
            },
        )
        .expect("Could not open GPUI window");
        cx.on_window_closed(|cx| cx.quit()).detach();
        cx.activate(true);
    });
}
