//! Pulse dashboard — the default landing view.
//!
//! Three vertical bands:
//!   1. cluster summary (counts only),
//!   2. trends (sparklines over the last ~60 samples),
//!   3. pinned resources (user-curated, gauge + status per pin).
//!
//! Sampling: one frame in roughly every `SAMPLE_INTERVAL` is treated
//! as a sample point — the rest of the frames just re-read counts so
//! the summary stays fresh.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use cruster_core::ResourceKey;
use cruster_kube::{MetricsCache, StoreRegistry};
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{Namespace, Node, Pod, Service};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::LoopState;
use crate::dashboard::{DashboardConfig, Pin};
use crate::overlays::search::Filter;
use crate::theme::Theme;
use crate::view::ResourceView;

const HISTORY_CAP: usize = 60;
const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

/// Pin tile cell size. Tiles auto-flow into 1/2/3+ columns based on
/// the pin-band width.
const TILE_W: u16 = 24;
const TILE_H: u16 = 6;
const TILE_GUTTER_X: u16 = 1;

#[derive(Debug, Default, Clone)]
struct Summary {
    /// kubelet version of the first node we saw, e.g. "v1.32.0".
    /// `None` if no nodes are in the registry (rare; before first
    /// watch sync). Used as a proxy for "cluster version" since we
    /// don't query the apiserver `/version` endpoint.
    k8s_version: Option<String>,
    nodes_total: usize,
    nodes_ready: usize,
    namespaces: usize,
    pods_total: usize,
    deployments_total: usize,
    services: usize,
}

#[derive(Debug, Default, Clone)]
struct Sample {
    /// Events whose `last_timestamp` is within the last 60s. Reads
    /// as a rough events-per-minute rate sampled once per second.
    events_per_min: u64,
    /// Cumulative container restart count across all pods at the
    /// time of the sample. Per-sample delta is what the chart
    /// actually plots (see `restart_deltas`).
    restarts_total: u64,
    /// Number of deployments where `ready_replicas < spec.replicas`
    /// — i.e. mid-rollout or stuck.
    rollouts_active: u64,
    /// One scalar per pin, parallel to `config.pins`. Pin index that
    /// doesn't fit the slot (e.g. the pin list grew) is missing.
    pin_values: Vec<u64>,
}

#[derive(Debug, Default, Clone)]
struct PinSnapshot {
    pod: Option<Pod>,
    deployment: Option<Deployment>,
    service: Option<Service>,
    node: Option<Node>,
    /// True iff lookup ran and found nothing (so the UI can show
    /// "missing" instead of "loading").
    looked_up: bool,
}

pub struct DashboardView {
    config: DashboardConfig,
    selected: usize,
    summary: Summary,
    /// Latest snapshot from the metrics-server poller. Cloned out of
    /// the registry-shared cache on each refresh so the render path
    /// doesn't have to await a lock.
    metrics: MetricsCache,
    history: VecDeque<Sample>,
    last_sample_at: Option<Instant>,
    pin_snapshots: Vec<PinSnapshot>,
    /// Kubeconfig current-context name, resolved once at construction.
    /// Used for the cluster-identity line in the summary band.
    context_name: String,
    /// Set by `handle_key`; consumed by `App` after `handle_key`
    /// returns so we can switch views without holding a mutable
    /// reference to `App` inside the view.
    pending_switch_id: Option<&'static str>,
    /// Unused on this view, but views must accept filters without
    /// crashing — keep the field so the trait method has somewhere to
    /// write.
    _filter: Filter,
}

impl Default for DashboardView {
    fn default() -> Self {
        Self {
            config: DashboardConfig::load_or_default(),
            selected: 0,
            summary: Summary::default(),
            metrics: MetricsCache::Initializing,
            history: VecDeque::with_capacity(HISTORY_CAP),
            last_sample_at: None,
            pin_snapshots: Vec::new(),
            context_name: current_kubeconfig_context(),
            pending_switch_id: None,
            _filter: Filter::default(),
        }
    }
}

impl DashboardView {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reload pins from disk. Called by `App` after a successful
    /// pin-to-dashboard so the dashboard reflects the new pin on the
    /// very next frame.
    pub fn reload_pins(&mut self) {
        self.config = DashboardConfig::load_or_default();
        if self.selected >= self.config.pins.len() {
            self.selected = self.config.pins.len().saturating_sub(1);
        }
        // Pin history slots no longer line up — reset per-pin
        // sparklines but keep the cluster trends.
        for sample in self.history.iter_mut() {
            sample.pin_values.clear();
        }
    }

    fn push_sample(&mut self, sample: Sample) {
        if self.history.len() == HISTORY_CAP {
            self.history.pop_front();
        }
        self.history.push_back(sample);
    }

}

/// How many tile columns fit in `width` accounting for gutters.
fn tile_cols_that_fit(width: u16) -> usize {
    if width < TILE_W {
        return 1;
    }
    ((width + TILE_GUTTER_X) / (TILE_W + TILE_GUTTER_X)).max(1) as usize
}

#[async_trait]
impl ResourceView for DashboardView {
    fn id(&self) -> &'static str {
        "dashboard"
    }

    async fn refresh(&mut self, registry: &StoreRegistry) {
        let pods = registry.pods.snapshot().await;
        let deployments = registry.deployments.snapshot().await;
        let services = registry.services.snapshot().await;
        let nodes = registry.nodes.snapshot().await;
        let namespaces = registry.namespaces.snapshot().await;
        let events = registry.events.snapshot().await;

        self.summary = summarise(&pods, &deployments, &services, &nodes, &namespaces, &events);
        self.metrics = registry.node_metrics.read().await.clone();

        // Resolve each pin against the live snapshots.
        self.pin_snapshots = self
            .config
            .pins
            .iter()
            .map(|pin| resolve_pin(pin, &pods, &deployments, &services, &nodes))
            .collect();

        // Sample tick (rate-limited to SAMPLE_INTERVAL).
        let now = Instant::now();
        let should_sample = match self.last_sample_at {
            None => true,
            Some(t) => now.duration_since(t) >= SAMPLE_INTERVAL,
        };
        if should_sample {
            self.last_sample_at = Some(now);
            let pin_values: Vec<u64> = self
                .config
                .pins
                .iter()
                .zip(self.pin_snapshots.iter())
                .map(|(pin, snap)| pin_scalar(pin, snap, &pods))
                .collect();
            self.push_sample(Sample {
                events_per_min: events_in_last_60s(&events),
                restarts_total: total_container_restarts(&pods),
                rollouts_active: deployments_rolling_out(&deployments),
                pin_values,
            });
        }

        if !self.config.pins.is_empty() && self.selected >= self.config.pins.len() {
            self.selected = self.config.pins.len() - 1;
        }
    }

    fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        // Summary + trends get their requested fixed lengths; pins
        // gets the remainder (down to a 2-row floor). The pin tile
        // grid already auto-flows and shows a "+N more" hint when
        // some tiles get clipped, so handing it the remainder rather
        // than `Min(pins_h)` keeps the cluster-identity numbers and
        // trends band from being squeezed on tall pin lists.
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                // Summary band: top border + identity line + scale
                // line + CPU bar + MEM bar + 1 padding = 6 rows.
                Constraint::Length(6),
                Constraint::Length(9), // trends band: top border + title + 6 chart rows + axis
                Constraint::Min(2),
            ])
            .split(area);

        self.render_summary(frame, chunks[0], theme);
        self.render_trends(frame, chunks[1], theme);
        self.render_pins(frame, chunks[2], theme);
    }

    fn handle_key(&mut self, key: KeyEvent) -> LoopState {
        if key.kind != KeyEventKind::Press {
            return LoopState::Continue;
        }
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                let n = self.config.pins.len();
                if n > 0 {
                    self.selected = (self.selected + 1).min(n - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Char('g') | KeyCode::Home => self.selected = 0,
            KeyCode::Char('G') | KeyCode::End => {
                self.selected = self.config.pins.len().saturating_sub(1);
            }
            KeyCode::Char('x') if !self.config.pins.is_empty() => {
                self.config.remove(self.selected);
                let _ = self.config.save();
                if self.selected >= self.config.pins.len() && self.selected > 0 {
                    self.selected -= 1;
                }
                for sample in self.history.iter_mut() {
                    sample.pin_values.clear();
                }
            }
            KeyCode::Enter => {
                if let Some(pin) = self.config.pins.get(self.selected) {
                    self.pending_switch_id = view_id_for_kind(&pin.kind);
                }
            }
            _ => {}
        }
        LoopState::Continue
    }

    fn selected_key(&self) -> Option<ResourceKey> {
        self.config.pins.get(self.selected).map(|p| p.to_key())
    }

    fn set_filter(&mut self, filter: Filter) {
        self._filter = filter;
    }

    fn take_pending_view_switch(&mut self) -> Option<&'static str> {
        self.pending_switch_id.take()
    }
}

impl DashboardView {
    /// Cluster identity card: who is this cluster, how big is it.
    /// Counts are slow-moving here; volatile numbers (event rate,
    /// restart rate, rollouts in flight) live in the trends band.
    fn render_summary(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let s = &self.summary;
        let version = s.k8s_version.as_deref().unwrap_or("?");

        // Line 1: cluster identity — name + version + node + namespace counts.
        let line1 = Line::from(vec![
            Span::styled("cluster ", Style::default().fg(theme.muted_fg.as_ratatui())),
            Span::styled(
                self.context_name.clone(),
                Style::default()
                    .fg(theme.header_fg.as_ratatui())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    " · {} · {}/{} nodes ready · {} ns",
                    version, s.nodes_ready, s.nodes_total, s.namespaces,
                ),
                Style::default().fg(theme.muted_fg.as_ratatui()),
            ),
        ]);

        // Line 2: scale — workload object counts. Movable but slow.
        let line2 = Line::from(vec![Span::styled(
            format!(
                "{} pods · {} deployments · {} services",
                s.pods_total, s.deployments_total, s.services
            ),
            Style::default().fg(theme.muted_fg.as_ratatui()),
        )]);

        // Lines 3-4: cluster CPU / MEM utilisation from metrics-server.
        // Drawn as ASCII bar gauges so they sit naturally in the
        // summary band's text flow without fighting the trends-band
        // chart aesthetic below.
        let (line3, line4) = utilisation_lines(&self.metrics, area.width, theme);

        let para = Paragraph::new(vec![line1, line2, line3, line4]).block(
            Block::default()
                .borders(Borders::TOP)
                .title(format!(" pulse · {} pins ", self.config.pins.len())),
        );
        frame.render_widget(para, area);
    }

    fn render_trends(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let block = Block::default().borders(Borders::TOP).title(" trends ");
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.height < 3 || inner.width < 30 {
            return;
        }

        // Three signals laid out horizontally as mini-cards. Each
        // card: title + big current value + multi-row BarChart of
        // recent samples + axis label.
        let events: Vec<u64> = self.history.iter().map(|s| s.events_per_min).collect();
        let restarts = restart_deltas(&self.history);
        let rollouts: Vec<u64> = self.history.iter().map(|s| s.rollouts_active).collect();

        // 3 cards + 2 single-column gutters between them.
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Ratio(1, 3),
                Constraint::Length(1),
                Constraint::Ratio(1, 3),
                Constraint::Length(1),
                Constraint::Ratio(1, 3),
            ])
            .split(inner);

        // Vertical separator glyphs in the gutters.
        for &gutter_idx in &[1usize, 3] {
            let g = cols[gutter_idx];
            for y in g.y..(g.y + g.height) {
                frame.render_widget(
                    Paragraph::new("│")
                        .style(Style::default().fg(theme.muted_fg.as_ratatui())),
                    Rect {
                        x: g.x,
                        y,
                        width: 1,
                        height: 1,
                    },
                );
            }
        }

        draw_trend_card(
            frame,
            cols[0],
            "events/min",
            &events,
            theme.sparkline.warn.as_ratatui(),
            theme,
        );
        draw_trend_card(
            frame,
            cols[2],
            "restarts/min",
            &restarts,
            theme.sparkline.danger.as_ratatui(),
            theme,
        );
        draw_trend_card(
            frame,
            cols[4],
            "rollouts",
            &rollouts,
            theme.sparkline.primary.as_ratatui(),
            theme,
        );
    }

    fn render_pins(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let block = Block::default()
            .borders(Borders::TOP)
            .title(" pinned · [a] add from any view · [x] unpin · [enter] open ");
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.config.pins.is_empty() {
            let hint = Paragraph::new(
                "no pins yet — switch to a list view (`:pods`, `:deploy`, …) and press `a`",
            )
            .style(Style::default().fg(theme.muted_fg.as_ratatui()));
            frame.render_widget(hint, inner);
            return;
        }

        if inner.height < TILE_H || inner.width < TILE_W {
            // Fallback for terminals too narrow / short for tiles.
            self.render_pins_compact(frame, inner, theme);
            return;
        }

        let cols = tile_cols_that_fit(inner.width);
        let max_rows = (inner.height / TILE_H) as usize;
        let visible = (cols * max_rows).min(self.config.pins.len());

        for i in 0..visible {
            let row = i / cols;
            let col = i % cols;
            let tile_rect = Rect {
                x: inner.x + (col as u16) * (TILE_W + TILE_GUTTER_X),
                y: inner.y + (row as u16) * TILE_H,
                width: TILE_W,
                height: TILE_H,
            };
            let pin = &self.config.pins[i];
            let snap = self.pin_snapshots.get(i);
            render_tile(frame, tile_rect, pin, snap, i == self.selected, theme);
        }

        if visible < self.config.pins.len() {
            let hidden = self.config.pins.len() - visible;
            let text = format!(" +{hidden} more (resize to show) ");
            let w = text.len() as u16;
            if inner.width > w {
                frame.render_widget(
                    Paragraph::new(text)
                        .style(Style::default().fg(theme.muted_fg.as_ratatui())),
                    Rect {
                        x: inner.x + inner.width - w,
                        y: inner.y + inner.height.saturating_sub(1),
                        width: w,
                        height: 1,
                    },
                );
            }
        }
    }

    /// Narrow-terminal fallback: one line per pin.
    fn render_pins_compact(&self, frame: &mut Frame<'_>, inner: Rect, theme: &Theme) {
        if inner.height == 0 {
            return;
        }
        for (i, pin) in self.config.pins.iter().enumerate() {
            let y = inner.y + i as u16;
            if y >= inner.y + inner.height {
                break;
            }
            let is_selected = i == self.selected;
            let marker = if is_selected { "▎" } else { " " };
            let style = if is_selected {
                Style::default()
                    .fg(theme.selection_fg.as_ratatui())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let snap = self.pin_snapshots.get(i);
            let summary = compact_pin_summary(pin, snap);
            frame.render_widget(
                Paragraph::new(format!("{marker} {} — {summary}", pin.label())).style(style),
                Rect {
                    x: inner.x,
                    y,
                    width: inner.width,
                    height: 1,
                },
            );
        }
    }
}

/// One trend mini-card. Layout (top-to-bottom):
///   row 0: title (left, muted) + current value (right, bold/colored)
///   rows 1..(h-2): vertical BarChart of `data` (one bar per sample)
///   row h-1: `60s ago  ─────────  now` axis label, muted
fn draw_trend_card(
    frame: &mut Frame<'_>,
    area: Rect,
    label: &str,
    data: &[u64],
    color: Color,
    theme: &Theme,
) {
    if area.height < 3 || area.width < 14 {
        return;
    }

    // Title row.
    let last = data.last().copied().unwrap_or(0);
    let value_text = format!("{last}");
    let value_w = value_text.len() as u16;
    frame.render_widget(
        Paragraph::new(label.to_string())
            .style(Style::default().fg(theme.muted_fg.as_ratatui())),
        Rect {
            x: area.x,
            y: area.y,
            width: area.width.saturating_sub(value_w + 1),
            height: 1,
        },
    );
    frame.render_widget(
        Paragraph::new(value_text)
            .alignment(ratatui::layout::Alignment::Right)
            .style(Style::default().fg(color).add_modifier(Modifier::BOLD)),
        Rect {
            x: area.x + area.width - value_w,
            y: area.y,
            width: value_w,
            height: 1,
        },
    );

    // Chart area: everything between title and axis label.
    let chart_h = area.height.saturating_sub(2);
    if chart_h > 0 && data.len() >= 2 {
        // BarChart with `bar_width=1` and `bar_gap=0` packs samples
        // tight; multi-row height gives visible height variation so
        // it doesn't read as a single progress bar.
        //
        // Use a chart-local max that gives the tallest bar a bit of
        // headroom — otherwise a single non-zero sample maxes the
        // chart and every other bar is invisibly short. A floor of
        // 5 keeps very-quiet clusters from showing every blip as
        // full-height.
        let observed_max = data.iter().copied().max().unwrap_or(0);
        let max_val = (observed_max + observed_max / 4 + 1).max(5);
        // `text_value("")` suppresses the default digit BarChart paints
        // at the base of every bar — with bar_width=1 it just becomes
        // visual noise (one garbled digit per column), and the
        // current-value readout in the title row already shows the
        // latest sample.
        let bars: Vec<ratatui::widgets::Bar> = data
            .iter()
            .map(|v| {
                ratatui::widgets::Bar::default()
                    .value(*v)
                    .text_value(String::new())
            })
            .collect();
        let chart = ratatui::widgets::BarChart::default()
            .data(ratatui::widgets::BarGroup::default().bars(&bars))
            .bar_width(1)
            .bar_gap(0)
            .max(max_val)
            .bar_style(Style::default().fg(color));
        frame.render_widget(
            chart,
            Rect {
                x: area.x,
                y: area.y + 1,
                width: area.width,
                height: chart_h,
            },
        );
    }

    // Axis row.
    let axis_y = area.y + area.height - 1;
    let axis_text = format_axis_label(area.width);
    frame.render_widget(
        Paragraph::new(axis_text).style(Style::default().fg(theme.muted_fg.as_ratatui())),
        Rect {
            x: area.x,
            y: axis_y,
            width: area.width,
            height: 1,
        },
    );
}

/// `60s ago ───── now` stretched to fit `width`.
fn format_axis_label(width: u16) -> String {
    let left = "60s ago";
    let right = "now";
    if (width as usize) < left.len() + right.len() + 2 {
        return "─".repeat(width as usize);
    }
    let dashes = (width as usize) - left.len() - right.len() - 2;
    format!("{left} {} {right}", "─".repeat(dashes))
}

/// Per-sample delta of `restarts_total`, normalised to roughly
/// restarts-per-minute. Sampling interval is 1s so the raw delta is
/// per-second; multiply by 60. Negative deltas (pod restart counter
/// resets after pod recreation) clamp to 0 so the chart doesn't
/// dip below the baseline.
fn restart_deltas(history: &VecDeque<Sample>) -> Vec<u64> {
    let mut out = Vec::with_capacity(history.len());
    let mut prev: Option<u64> = None;
    for s in history {
        let delta = match prev {
            Some(p) if s.restarts_total >= p => (s.restarts_total - p) * 60,
            _ => 0,
        };
        out.push(delta);
        prev = Some(s.restarts_total);
    }
    out
}

/// Per-tile visual shape. Numeric kinds (Deployment) get a car-
/// style gauge — arc, needle, tick scale, value. Status kinds get
/// big centered text + sub label.
enum TileVisual {
    Gauge {
        percent: u8,
        value_text: String,
        sub: String,
        color: Color,
    },
    Status {
        big: String,
        sub: String,
        color: Color,
    },
    Missing,
    Loading,
}

fn tile_visual(pin: &Pin, snap: Option<&PinSnapshot>, theme: &Theme) -> TileVisual {
    let Some(snap) = snap else {
        return TileVisual::Loading;
    };
    if snap.looked_up
        && snap.pod.is_none()
        && snap.deployment.is_none()
        && snap.service.is_none()
        && snap.node.is_none()
    {
        return TileVisual::Missing;
    }
    match pin.kind.as_str() {
        "Deployment" => deployment_visual(snap.deployment.as_ref(), theme),
        "Pod" => pod_visual(snap.pod.as_ref(), theme),
        "Service" => service_visual(snap.service.as_ref(), theme),
        "Node" => node_visual(snap.node.as_ref(), theme),
        other => TileVisual::Status {
            big: other.into(),
            sub: "pinned".into(),
            color: theme.muted_fg.as_ratatui(),
        },
    }
}

fn deployment_visual(deploy: Option<&Deployment>, theme: &Theme) -> TileVisual {
    let Some(d) = deploy else {
        return TileVisual::Loading;
    };
    let desired = d.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0).max(0);
    let ready = d
        .status
        .as_ref()
        .and_then(|s| s.ready_replicas)
        .unwrap_or(0)
        .max(0);
    let pct = if desired == 0 {
        0
    } else {
        ((ready as f64 / desired as f64) * 100.0)
            .round()
            .clamp(0.0, 100.0) as u8
    };
    let color = if desired == 0 {
        theme.muted_fg.as_ratatui()
    } else if ready >= desired {
        theme.gauge.ok.as_ratatui()
    } else if ready == 0 {
        theme.gauge.danger.as_ratatui()
    } else {
        theme.gauge.warn.as_ratatui()
    };
    TileVisual::Gauge {
        percent: pct,
        value_text: format!("{pct}%"),
        sub: format!("{ready}/{desired} ready"),
        color,
    }
}

fn pod_visual(pod: Option<&Pod>, theme: &Theme) -> TileVisual {
    let Some(p) = pod else {
        return TileVisual::Loading;
    };
    let phase = p
        .status
        .as_ref()
        .and_then(|s| s.phase.clone())
        .unwrap_or_else(|| "?".into());
    let (ready, total) = container_ready_counts(p);
    let restarts: i32 = p
        .status
        .as_ref()
        .and_then(|s| s.container_statuses.as_ref())
        .map(|cs| cs.iter().map(|c| c.restart_count).sum())
        .unwrap_or(0);
    // Gauge value = container ready ratio. Color combines phase +
    // ratio so a 0/1 Failed pod is unmistakably red and a 1/1
    // Running pod is green, even though the underlying number
    // could be the same (100%).
    let percent = if total > 0 {
        ((ready as f64 / total as f64) * 100.0).round() as u8
    } else {
        0
    };
    let color = match phase.as_str() {
        "Failed" => theme.gauge.danger.as_ratatui(),
        "Pending" => theme.gauge.warn.as_ratatui(),
        "Succeeded" => theme.muted_fg.as_ratatui(),
        "Running" if percent >= 100 => theme.gauge.ok.as_ratatui(),
        "Running" => theme.gauge.warn.as_ratatui(),
        _ => theme.muted_fg.as_ratatui(),
    };
    TileVisual::Gauge {
        percent,
        value_text: phase,
        sub: format!("{ready}/{total} ready · {restarts} restarts"),
        color,
    }
}

fn service_visual(svc: Option<&Service>, theme: &Theme) -> TileVisual {
    let Some(s) = svc else {
        return TileVisual::Loading;
    };
    let svc_type = s
        .spec
        .as_ref()
        .and_then(|sp| sp.type_.clone())
        .unwrap_or_else(|| "?".into());
    let cluster_ip = s
        .spec
        .as_ref()
        .and_then(|sp| sp.cluster_ip.clone())
        .unwrap_or_else(|| "-".into());
    let ports = s
        .spec
        .as_ref()
        .and_then(|sp| sp.ports.as_ref())
        .map(|p| p.len())
        .unwrap_or(0);
    TileVisual::Status {
        big: svc_type,
        sub: format!(
            "{cluster_ip} · {ports} port{}",
            if ports == 1 { "" } else { "s" }
        ),
        color: theme.header_fg.as_ratatui(),
    }
}

fn node_visual(node: Option<&Node>, theme: &Theme) -> TileVisual {
    let Some(n) = node else {
        return TileVisual::Loading;
    };
    let ready = node_is_ready(n);
    let (status, color, percent) = if ready {
        ("Ready", theme.gauge.ok.as_ratatui(), 100u8)
    } else {
        ("NotReady", theme.gauge.danger.as_ratatui(), 0u8)
    };
    TileVisual::Gauge {
        percent,
        value_text: status.into(),
        sub: String::new(),
        color,
    }
}

/// A pin tile. Bordered cell with a title and a body whose shape
/// depends on the kind. Numeric kinds (Deployment) get a car-style
/// gauge. Status kinds get big text + sub-label.
fn render_tile(
    frame: &mut Frame<'_>,
    area: Rect,
    pin: &Pin,
    snap: Option<&PinSnapshot>,
    selected: bool,
    theme: &Theme,
) {
    let visual = tile_visual(pin, snap, theme);
    let border_style = if selected {
        Style::default()
            .fg(theme.selection_fg.as_ratatui())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.muted_fg.as_ratatui())
    };
    let title = format!(
        " {} ",
        truncate_label(&pin.label(), area.width.saturating_sub(4))
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    match visual {
        TileVisual::Gauge {
            percent,
            value_text,
            sub,
            color,
        } => render_gauge(frame, inner, percent, &value_text, &sub, color, theme),
        TileVisual::Status { big, sub, color } => {
            render_status(frame, inner, &big, &sub, color, theme)
        }
        TileVisual::Missing => render_status(
            frame,
            inner,
            "(missing)",
            "",
            theme.status.failed.as_ratatui(),
            theme,
        ),
        TileVisual::Loading => {
            render_status(frame, inner, "…", "", theme.muted_fg.as_ratatui(), theme)
        }
    }
}

/// Dial-style gauge inside a tile body. Four rows:
///   row 0: arc top (dotted, coloured by zone)
///   row 1: needle ▼ positioned by percent
///   row 2: tick axis (5 major ticks, muted)
///   row 3: value readout (centered, bold, coloured by zone)
fn render_gauge(
    frame: &mut Frame<'_>,
    inner: Rect,
    percent: u8,
    value_text: &str,
    sub: &str,
    color: Color,
    theme: &Theme,
) {
    if inner.height < 1 {
        return;
    }
    let pad = 1u16;
    if inner.width < pad * 2 + 3 {
        render_status(frame, inner, value_text, sub, color, theme);
        return;
    }
    let arc_x = inner.x + pad;
    let arc_w = inner.width - pad * 2;

    // ARC TOP
    let mut arc = String::with_capacity(arc_w as usize);
    arc.push('╭');
    for _ in 1..(arc_w - 1) {
        arc.push('┄');
    }
    arc.push('╮');
    frame.render_widget(
        Paragraph::new(arc).style(Style::default().fg(color)),
        Rect {
            x: arc_x,
            y: inner.y,
            width: arc_w,
            height: 1,
        },
    );
    if inner.height < 2 {
        return;
    }

    // NEEDLE
    let span = arc_w.saturating_sub(2).max(1);
    let needle_offset = ((percent as u32) * (span as u32 - 1) / 100) as u16;
    let needle_x = arc_x + 1 + needle_offset;
    frame.render_widget(
        Paragraph::new("▼").style(Style::default().fg(color).add_modifier(Modifier::BOLD)),
        Rect {
            x: needle_x,
            y: inner.y + 1,
            width: 1,
            height: 1,
        },
    );
    if inner.height < 3 {
        return;
    }

    // TICK AXIS
    if arc_w >= 5 {
        let mut ticks = vec![' '; arc_w as usize];
        for k in 0..5u16 {
            let idx = ((k as u32 * (arc_w as u32 - 1)) / 4) as usize;
            ticks[idx] = '┴';
        }
        for (i, c) in ticks.iter_mut().enumerate() {
            if *c == ' ' && i > 0 && i < arc_w as usize - 1 {
                *c = '─';
            }
        }
        let tick_str: String = ticks.into_iter().collect();
        frame.render_widget(
            Paragraph::new(tick_str).style(Style::default().fg(theme.muted_fg.as_ratatui())),
            Rect {
                x: arc_x,
                y: inner.y + 2,
                width: arc_w,
                height: 1,
            },
        );
    }
    if inner.height < 4 {
        return;
    }

    // VALUE READOUT (centered, bold, coloured)
    frame.render_widget(
        Paragraph::new(truncate_label(value_text, inner.width))
            .alignment(ratatui::layout::Alignment::Center)
            .style(Style::default().fg(color).add_modifier(Modifier::BOLD)),
        Rect {
            x: inner.x,
            y: inner.y + 3,
            width: inner.width,
            height: 1,
        },
    );
}

/// Status-only tile body: big text + sub-label, both centered.
fn render_status(
    frame: &mut Frame<'_>,
    inner: Rect,
    big: &str,
    sub: &str,
    color: Color,
    theme: &Theme,
) {
    if inner.height == 0 {
        return;
    }
    let pair = if sub.is_empty() { 1 } else { 2 };
    let big_row = inner.y + inner.height.saturating_sub(pair) / 2;
    let sub_row = big_row + 1;

    frame.render_widget(
        Paragraph::new(truncate_label(big, inner.width))
            .alignment(ratatui::layout::Alignment::Center)
            .style(Style::default().fg(color).add_modifier(Modifier::BOLD)),
        Rect {
            x: inner.x,
            y: big_row,
            width: inner.width,
            height: 1,
        },
    );

    if !sub.is_empty() && sub_row < inner.y + inner.height {
        frame.render_widget(
            Paragraph::new(truncate_label(sub, inner.width))
                .alignment(ratatui::layout::Alignment::Center)
                .style(Style::default().fg(theme.muted_fg.as_ratatui())),
            Rect {
                x: inner.x,
                y: sub_row,
                width: inner.width,
                height: 1,
            },
        );
    }
}

fn compact_pin_summary(pin: &Pin, snap: Option<&PinSnapshot>) -> String {
    // Theme-independent text-only summary for the fallback row layout.
    let stub_theme = crate::theme::Theme::terminal_default();
    match tile_visual(pin, snap, &stub_theme) {
        TileVisual::Gauge {
            value_text, sub, ..
        } => {
            if sub.is_empty() {
                value_text
            } else {
                format!("{value_text} · {sub}")
            }
        }
        TileVisual::Status { big, sub, .. } => {
            if sub.is_empty() {
                big
            } else {
                format!("{big} · {sub}")
            }
        }
        TileVisual::Missing => "(missing)".into(),
        TileVisual::Loading => "…".into(),
    }
}

/// Build the two utilisation lines that sit under the scale line in
/// the summary band. Returns `(cpu_line, mem_line)`. When metrics-
/// server isn't available, both lines collapse: the CPU slot carries
/// the muted fallback message and the MEM slot is blank.
fn utilisation_lines<'a>(
    cache: &MetricsCache,
    area_width: u16,
    theme: &Theme,
) -> (Line<'a>, Line<'a>) {
    let muted = Style::default().fg(theme.muted_fg.as_ratatui());
    match cache {
        MetricsCache::Initializing => (
            Line::from(Span::styled("metrics-server …", muted)),
            Line::from(""),
        ),
        MetricsCache::Unavailable { reason } => (
            Line::from(Span::styled(reason.clone(), muted)),
            Line::from(""),
        ),
        MetricsCache::Available {
            cpu_used_cores,
            cpu_capacity_cores,
            mem_used_bytes,
            mem_capacity_bytes,
            ..
        } => {
            let bar_w = bar_width_for(area_width);
            let cpu = build_utilisation_line(
                "CPU",
                *cpu_used_cores,
                *cpu_capacity_cores,
                &format!(
                    "{} / {} cores",
                    format_cores(*cpu_used_cores),
                    format_cores(*cpu_capacity_cores)
                ),
                bar_w,
                theme,
            );
            let mem = build_utilisation_line(
                "MEM",
                *mem_used_bytes as f64,
                *mem_capacity_bytes as f64,
                &format!(
                    "{} / {}",
                    format_bytes(*mem_used_bytes),
                    format_bytes(*mem_capacity_bytes)
                ),
                bar_w,
                theme,
            );
            (cpu, mem)
        }
    }
}

/// Width of the ASCII bar based on terminal width. Aims for a bar
/// that's roughly half the row, leaving room for the label, percent,
/// and `used / capacity` readout.
fn bar_width_for(area_width: u16) -> usize {
    // Leave ~36 chars for the label / percent / readout suffixes.
    let reserved = 36u16;
    let candidate = area_width.saturating_sub(reserved);
    candidate.clamp(8, 30) as usize
}

fn build_utilisation_line<'a>(
    label: &str,
    used: f64,
    capacity: f64,
    suffix: &str,
    bar_w: usize,
    theme: &Theme,
) -> Line<'a> {
    let pct = if capacity > 0.0 {
        ((used / capacity) * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };
    let pct_u = pct.round() as u8;
    let zone_color = utilisation_zone_color(pct_u, theme);
    let bar = ascii_bar(pct_u, bar_w);
    Line::from(vec![
        Span::styled(
            format!("{label}  "),
            Style::default().fg(theme.muted_fg.as_ratatui()),
        ),
        Span::styled(
            format!("{:>3}%  ", pct_u),
            Style::default()
                .fg(zone_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(bar, Style::default().fg(zone_color)),
        Span::styled(
            format!("  {suffix}"),
            Style::default().fg(theme.muted_fg.as_ratatui()),
        ),
    ])
}

/// Filled/empty unicode block bar. `█` for filled, `░` for empty.
fn ascii_bar(pct: u8, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let filled = (pct as usize * width).div_ceil(100).min(width);
    let mut s = String::with_capacity(width * 3);
    for _ in 0..filled {
        s.push('█');
    }
    for _ in 0..(width - filled) {
        s.push('░');
    }
    s
}

/// Zone colour for a 0-100 utilisation percentage. Matches the
/// "ok / warn / danger" idiom from `theme.gauge` used by pin tiles.
fn utilisation_zone_color(pct: u8, theme: &Theme) -> Color {
    if pct >= 90 {
        theme.gauge.danger.as_ratatui()
    } else if pct >= 75 {
        theme.gauge.warn.as_ratatui()
    } else {
        theme.gauge.ok.as_ratatui()
    }
}

/// Format CPU cores with sensible precision: integer cores when the
/// value is large, two decimal places when small (so we don't show
/// "0" for a sub-core sample).
fn format_cores(cores: f64) -> String {
    if cores >= 10.0 {
        format!("{:.0}", cores)
    } else {
        format!("{:.2}", cores)
    }
}

/// Format a byte count using binary (1024) units, like `kubectl top`.
fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else if v >= 100.0 {
        format!("{v:.0} {}", UNITS[i])
    } else if v >= 10.0 {
        format!("{v:.1} {}", UNITS[i])
    } else {
        format!("{v:.2} {}", UNITS[i])
    }
}

fn truncate_label(s: &str, max_chars: u16) -> String {
    let max = max_chars as usize;
    if s.chars().count() <= max {
        return s.into();
    }
    if max <= 1 {
        return "…".into();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}


fn summarise(
    pods: &[(ResourceKey, Pod)],
    deployments: &[(ResourceKey, Deployment)],
    services: &[(ResourceKey, Service)],
    nodes: &[(ResourceKey, Node)],
    namespaces: &[(ResourceKey, Namespace)],
    events: &[(ResourceKey, k8s_openapi::api::core::v1::Event)],
) -> Summary {
    let _ = events; // event rate now lives in the trends band
    let nodes_total = nodes.len();
    let nodes_ready = nodes.iter().filter(|(_, n)| node_is_ready(n)).count();
    let k8s_version = nodes.iter().find_map(|(_, n)| {
        n.status
            .as_ref()
            .and_then(|s| s.node_info.as_ref())
            .map(|info| info.kubelet_version.clone())
    });
    Summary {
        k8s_version,
        nodes_total,
        nodes_ready,
        namespaces: namespaces.len(),
        pods_total: pods.len(),
        deployments_total: deployments.len(),
        services: services.len(),
    }
}

fn resolve_pin(
    pin: &Pin,
    pods: &[(ResourceKey, Pod)],
    deployments: &[(ResourceKey, Deployment)],
    services: &[(ResourceKey, Service)],
    nodes: &[(ResourceKey, Node)],
) -> PinSnapshot {
    let mut out = PinSnapshot {
        looked_up: true,
        ..PinSnapshot::default()
    };
    match pin.kind.as_str() {
        "Pod" => {
            out.pod = pods
                .iter()
                .find(|(k, _)| pin.matches(k))
                .map(|(_, v)| v.clone());
        }
        "Deployment" => {
            out.deployment = deployments
                .iter()
                .find(|(k, _)| pin.matches(k))
                .map(|(_, v)| v.clone());
        }
        "Service" => {
            out.service = services
                .iter()
                .find(|(k, _)| pin.matches(k))
                .map(|(_, v)| v.clone());
        }
        "Node" => {
            out.node = nodes
                .iter()
                .find(|(k, _)| pin.matches(k))
                .map(|(_, v)| v.clone());
        }
        _ => {}
    }
    out
}

fn pin_scalar(pin: &Pin, snap: &PinSnapshot, pods: &[(ResourceKey, Pod)]) -> u64 {
    match pin.kind.as_str() {
        "Deployment" => snap
            .deployment
            .as_ref()
            .and_then(|d| d.status.as_ref())
            .and_then(|s| s.ready_replicas)
            .unwrap_or(0)
            .max(0) as u64,
        "Pod" => snap
            .pod
            .as_ref()
            .and_then(|p| p.status.as_ref())
            .and_then(|s| s.container_statuses.as_ref())
            .map(|cs| cs.iter().map(|c| c.restart_count as u64).sum())
            .unwrap_or(0),
        "Service" => snap
            .service
            .as_ref()
            .and_then(|s| s.spec.as_ref())
            .and_then(|sp| sp.ports.as_ref())
            .map(|p| p.len() as u64)
            .unwrap_or(0),
        "Node" => pods
            .iter()
            .filter(|(_, p)| {
                p.spec
                    .as_ref()
                    .and_then(|s| s.node_name.as_deref())
                    .map(|n| n == pin.name)
                    .unwrap_or(false)
            })
            .count() as u64,
        _ => 0,
    }
}

fn container_ready_counts(pod: &Pod) -> (usize, usize) {
    let cs = pod
        .status
        .as_ref()
        .and_then(|s| s.container_statuses.as_ref())
        .map(|c| c.as_slice())
        .unwrap_or(&[]);
    (cs.iter().filter(|c| c.ready).count(), cs.len())
}

fn current_kubeconfig_context() -> String {
    kube::config::Kubeconfig::read()
        .ok()
        .and_then(|c| c.current_context)
        .unwrap_or_else(|| "?".into())
}

fn node_is_ready(node: &Node) -> bool {
    node.status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .map(|conds| {
            conds
                .iter()
                .any(|c| c.type_ == "Ready" && c.status == "True")
        })
        .unwrap_or(false)
}

fn events_in_last_60s(events: &[(ResourceKey, k8s_openapi::api::core::v1::Event)]) -> u64 {
    let cutoff = chrono::Utc::now() - chrono::Duration::seconds(60);
    events
        .iter()
        .filter(|(_, e)| {
            e.last_timestamp
                .as_ref()
                .map(|t| t.0 >= cutoff)
                .unwrap_or(false)
        })
        .count() as u64
}

fn total_container_restarts(pods: &[(ResourceKey, Pod)]) -> u64 {
    let mut total: i64 = 0;
    for (_, p) in pods {
        if let Some(cs) = p
            .status
            .as_ref()
            .and_then(|s| s.container_statuses.as_ref())
        {
            for c in cs {
                total += c.restart_count as i64;
            }
        }
    }
    total.max(0) as u64
}

fn deployments_rolling_out(deployments: &[(ResourceKey, Deployment)]) -> u64 {
    deployments
        .iter()
        .filter(|(_, d)| {
            let desired = d.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0);
            let ready = d
                .status
                .as_ref()
                .and_then(|s| s.ready_replicas)
                .unwrap_or(0);
            desired > 0 && ready < desired
        })
        .count() as u64
}

fn view_id_for_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "Pod" => Some("pods"),
        "Deployment" => Some("deployments"),
        "Service" => Some("services"),
        "Node" => Some("nodes"),
        "Namespace" => Some("namespaces"),
        "ConfigMap" => Some("configmaps"),
        "Secret" => Some("secrets"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cruster_kube::StoreRegistry;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    fn make_pod(ns: &str, name: &str, phase: &str) -> (ResourceKey, Pod) {
        let pod = Pod {
            metadata: ObjectMeta {
                name: Some(name.into()),
                namespace: Some(ns.into()),
                ..Default::default()
            },
            status: Some(k8s_openapi::api::core::v1::PodStatus {
                phase: Some(phase.into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        (ResourceKey::namespaced("Pod", ns, name), pod)
    }

    #[tokio::test]
    async fn refresh_pushes_first_sample() {
        let registry = StoreRegistry::new();
        let (k, p) = make_pod("default", "a", "Running");
        registry.pods.upsert(k, p).await;
        let mut v = DashboardView::new();
        v.refresh(&registry).await;
        assert_eq!(v.history.len(), 1);
        assert_eq!(v.summary.pods_total, 1);
    }

    #[tokio::test]
    async fn refresh_rate_limits_samples() {
        let registry = StoreRegistry::new();
        let mut v = DashboardView::new();
        v.refresh(&registry).await;
        v.refresh(&registry).await;
        v.refresh(&registry).await;
        // Three quick refreshes within <1s should produce one sample.
        assert_eq!(v.history.len(), 1);
    }

    #[test]
    fn render_trend_label_uses_themes_muted_fg() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut v = DashboardView::new();
        v.history.push_back(Sample {
            events_per_min: 1,
            restarts_total: 0,
            rollouts_active: 0,
            pin_values: vec![],
        });

        let theme = crate::theme::Theme::embedded("solarized-light").unwrap();
        let want_muted = theme.muted_fg.as_ratatui();

        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| v.render(f, f.area(), &theme)).unwrap();
        let buf = terminal.backend().buffer();

        let mut row_text = String::new();
        let mut row_fg = None;
        for y in 0..buf.area().height {
            let mut line = String::new();
            for x in 0..buf.area().width {
                line.push_str(buf[(x, y)].symbol());
            }
            if line.contains("events/min") {
                row_text = line;
                // The label's first 'e' should carry the muted_fg color.
                for x in 0..buf.area().width {
                    let cell = &buf[(x, y)];
                    if cell.symbol() == "e" {
                        row_fg = cell.style().fg;
                        break;
                    }
                }
                break;
            }
        }
        assert!(
            !row_text.is_empty(),
            "expected to find an 'events/min' trend label"
        );
        assert_eq!(
            row_fg,
            Some(want_muted),
            "trend label should be painted in theme.muted_fg"
        );
    }

    #[test]
    fn view_id_for_kind_maps_known_kinds() {
        assert_eq!(view_id_for_kind("Pod"), Some("pods"));
        assert_eq!(view_id_for_kind("Deployment"), Some("deployments"));
        assert_eq!(view_id_for_kind("CustomResource"), None);
    }

    #[test]
    fn render_metrics_available_shows_cpu_and_mem_lines() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use std::time::SystemTime;

        let mut v = DashboardView::new();
        v.metrics = MetricsCache::Available {
            cpu_used_cores: 1.5,
            cpu_capacity_cores: 4.0,
            mem_used_bytes: 2 * 1024 * 1024 * 1024,
            mem_capacity_bytes: 8 * 1024 * 1024 * 1024,
            sampled_at: SystemTime::now(),
        };
        let theme = crate::theme::Theme::terminal_default();
        let backend = TestBackend::new(80, 16);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| v.render(f, f.area(), &theme)).unwrap();
        let buf = terminal.backend().buffer();

        let text = buffer_to_string(buf);
        assert!(text.contains("CPU"), "expected CPU label in:\n{text}");
        assert!(text.contains("MEM"), "expected MEM label in:\n{text}");
        // 1.5 / 4 cores = 38% CPU, 2 / 8 GiB = 25% MEM.
        assert!(text.contains("38%"), "expected 38% in:\n{text}");
        assert!(text.contains("25%"), "expected 25% in:\n{text}");
        assert!(
            text.contains("1.50 / 4.00 cores"),
            "expected cores readout in:\n{text}"
        );
        assert!(text.contains("GiB"), "expected GiB readout in:\n{text}");
    }

    #[test]
    fn render_metrics_unavailable_shows_muted_fallback() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut v = DashboardView::new();
        v.metrics = MetricsCache::Unavailable {
            reason: "metrics-server not installed".into(),
        };
        let theme = crate::theme::Theme::terminal_default();
        let backend = TestBackend::new(80, 16);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| v.render(f, f.area(), &theme)).unwrap();
        let buf = terminal.backend().buffer();

        let text = buffer_to_string(buf);
        assert!(
            text.contains("metrics-server not installed"),
            "expected fallback message in:\n{text}"
        );
        // No percent value should be rendered when unavailable.
        assert!(!text.contains('%'), "did not expect a % readout in:\n{text}");
    }

    fn buffer_to_string(buf: &ratatui::buffer::Buffer) -> String {
        let mut out = String::new();
        for y in 0..buf.area().height {
            for x in 0..buf.area().width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn ascii_bar_widths_round_to_filled() {
        assert_eq!(ascii_bar(0, 10), "░░░░░░░░░░");
        assert_eq!(ascii_bar(100, 10), "██████████");
        assert_eq!(ascii_bar(50, 10), "█████░░░░░");
        // 1% in a 10-cell bar should still show at least 1 filled cell.
        assert_eq!(ascii_bar(1, 10).chars().next(), Some('█'));
    }

    #[test]
    fn format_bytes_uses_binary_units() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.00 KiB");
        assert_eq!(format_bytes(1024 * 1024), "1.00 MiB");
        assert_eq!(format_bytes(2 * 1024 * 1024 * 1024), "2.00 GiB");
        assert_eq!(format_bytes(150 * 1024), "150 KiB");
    }

    #[test]
    fn format_cores_picks_appropriate_precision() {
        assert_eq!(format_cores(0.05), "0.05");
        assert_eq!(format_cores(1.5), "1.50");
        assert_eq!(format_cores(42.0), "42");
    }

    #[tokio::test]
    async fn refresh_summary_counts_phases() {
        let registry = StoreRegistry::new();
        for (k, p) in [
            make_pod("default", "a", "Running"),
            make_pod("default", "b", "Running"),
            make_pod("default", "c", "Pending"),
            make_pod("default", "d", "Failed"),
        ] {
            registry.pods.upsert(k, p).await;
        }
        let mut v = DashboardView::new();
        v.refresh(&registry).await;
        // The phase-bucket fields were folded into the trends band;
        // the summary now just carries the totals.
        assert_eq!(v.summary.pods_total, 4);
    }
}
