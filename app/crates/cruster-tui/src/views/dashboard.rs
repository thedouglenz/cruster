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
use cruster_kube::StoreRegistry;
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{Namespace, Node, Pod, Service};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Sparkline};
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
    nodes_total: usize,
    nodes_ready: usize,
    namespaces: usize,
    pods_total: usize,
    pods_running: usize,
    pods_pending: usize,
    pods_failed: usize,
    pods_succeeded: usize,
    deployments_total: usize,
    deployments_ready: usize,
    services: usize,
    events_recent: usize,
}

#[derive(Debug, Default, Clone)]
struct Sample {
    pods_running: u64,
    pods_failed: u64,
    events_recent: u64,
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
    history: VecDeque<Sample>,
    last_sample_at: Option<Instant>,
    pin_snapshots: Vec<PinSnapshot>,
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
            history: VecDeque::with_capacity(HISTORY_CAP),
            last_sample_at: None,
            pin_snapshots: Vec::new(),
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

    /// How tall the pins band wants to be, given current width.
    /// Driven by the tile grid that fits at this width.
    fn pins_band_height(&self, available_width: u16) -> u16 {
        if self.config.pins.is_empty() {
            return 2;
        }
        let cols = tile_cols_that_fit(available_width).max(1);
        let rows = self.config.pins.len().div_ceil(cols);
        (rows as u16).saturating_mul(TILE_H) + 1
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
                pods_running: self.summary.pods_running as u64,
                pods_failed: self.summary.pods_failed as u64,
                events_recent: self.summary.events_recent as u64,
                pin_values,
            });
        }

        if !self.config.pins.is_empty() && self.selected >= self.config.pins.len() {
            self.selected = self.config.pins.len() - 1;
        }
    }

    fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        // Pins band wants as much room as its tile grid needs, but
        // never more than half the screen — trends + summary still
        // matter. Floor at TILE_H+1 so at least one tile-row fits.
        let pins_h = self
            .pins_band_height(area.width)
            .min(area.height / 2)
            .max(if self.config.pins.is_empty() {
                2
            } else {
                TILE_H + 1
            });
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4), // summary band (top border + 2 lines + 1 padding)
                Constraint::Min(5),    // trends band fills remainder
                Constraint::Length(pins_h),
            ])
            .split(area);

        self.render_summary(frame, chunks[0]);
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
    fn render_summary(&self, frame: &mut Frame<'_>, area: Rect) {
        let s = &self.summary;
        let line1 = format!(
            "nodes {}/{} ready · ns {} · pods {} (R{} P{} F{} S{})",
            s.nodes_ready,
            s.nodes_total,
            s.namespaces,
            s.pods_total,
            s.pods_running,
            s.pods_pending,
            s.pods_failed,
            s.pods_succeeded,
        );
        let line2 = format!(
            "deploys {}/{} ready · svc {} · events {} recent",
            s.deployments_ready, s.deployments_total, s.services, s.events_recent,
        );
        let para = Paragraph::new(vec![Line::from(line1), Line::from(line2)]).block(
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

        if inner.height < 2 {
            return;
        }

        let pods_running: Vec<u64> = self.history.iter().map(|s| s.pods_running).collect();
        let pods_failed: Vec<u64> = self.history.iter().map(|s| s.pods_failed).collect();
        let events_recent: Vec<u64> = self.history.iter().map(|s| s.events_recent).collect();

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .split(inner);

        draw_trend_row(
            frame,
            rows[0],
            "pods running",
            &pods_running,
            theme.sparkline.primary.as_ratatui(),
            theme,
        );
        draw_trend_row(
            frame,
            rows[1],
            "pods failed ",
            &pods_failed,
            theme.sparkline.danger.as_ratatui(),
            theme,
        );
        draw_trend_row(
            frame,
            rows[2],
            "events      ",
            &events_recent,
            theme.sparkline.warn.as_ratatui(),
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

fn draw_trend_row(
    frame: &mut Frame<'_>,
    area: Rect,
    label: &str,
    data: &[u64],
    color: Color,
    theme: &Theme,
) {
    if area.width < (label.len() as u16) + 8 {
        return;
    }
    let last = data.last().copied().unwrap_or(0);
    let label_rect = Rect {
        x: area.x,
        y: area.y,
        width: label.len() as u16,
        height: 1,
    };
    let spark_w = area
        .width
        .saturating_sub(label_rect.width)
        .saturating_sub(6);
    let spark_rect = Rect {
        x: label_rect.x + label_rect.width + 1,
        y: area.y,
        width: spark_w,
        height: 1,
    };
    let val_rect = Rect {
        x: spark_rect.x + spark_rect.width + 1,
        y: area.y,
        width: 4,
        height: 1,
    };
    frame.render_widget(
        Paragraph::new(label.to_string()).style(Style::default().fg(theme.muted_fg.as_ratatui())),
        label_rect,
    );
    let spark = Sparkline::default()
        .data(data)
        .style(Style::default().fg(color));
    frame.render_widget(spark, spark_rect);
    frame.render_widget(
        Paragraph::new(format!("{last:>4}")).style(Style::default().fg(color)),
        val_rect,
    );
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
    TileVisual::Status {
        big: phase.clone(),
        sub: format!("{ready}/{total} ready · {restarts} restarts"),
        color: phase_color_for(&phase, theme),
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
    let (status, color) = if ready {
        ("Ready", theme.status.running.as_ratatui())
    } else {
        ("NotReady", theme.status.failed.as_ratatui())
    };
    TileVisual::Status {
        big: status.into(),
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

fn phase_color_for(phase: &str, theme: &Theme) -> Color {
    match phase {
        "Running" => theme.status.running.as_ratatui(),
        "Pending" => theme.status.pending.as_ratatui(),
        "Failed" => theme.status.failed.as_ratatui(),
        "Succeeded" => theme.status.succeeded.as_ratatui(),
        _ => theme.status.unknown.as_ratatui(),
    }
}

fn summarise(
    pods: &[(ResourceKey, Pod)],
    deployments: &[(ResourceKey, Deployment)],
    services: &[(ResourceKey, Service)],
    nodes: &[(ResourceKey, Node)],
    namespaces: &[(ResourceKey, Namespace)],
    events: &[(ResourceKey, k8s_openapi::api::core::v1::Event)],
) -> Summary {
    let nodes_total = nodes.len();
    let nodes_ready = nodes.iter().filter(|(_, n)| node_is_ready(n)).count();
    let namespaces_count = namespaces.len();

    let mut pods_running = 0;
    let mut pods_pending = 0;
    let mut pods_failed = 0;
    let mut pods_succeeded = 0;
    for (_, p) in pods {
        match p
            .status
            .as_ref()
            .and_then(|s| s.phase.as_deref())
            .unwrap_or("")
        {
            "Running" => pods_running += 1,
            "Pending" => pods_pending += 1,
            "Failed" => pods_failed += 1,
            "Succeeded" => pods_succeeded += 1,
            _ => {}
        }
    }
    let deployments_total = deployments.len();
    let deployments_ready = deployments
        .iter()
        .filter(|(_, d)| {
            let desired = d.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0);
            let ready = d
                .status
                .as_ref()
                .and_then(|s| s.ready_replicas)
                .unwrap_or(0);
            desired > 0 && ready >= desired
        })
        .count();
    let services_count = services.len();

    let cutoff = chrono::Utc::now() - chrono::Duration::minutes(5);
    let events_recent = events
        .iter()
        .filter(|(_, e)| {
            e.last_timestamp
                .as_ref()
                .map(|t| t.0 >= cutoff)
                .unwrap_or(false)
        })
        .count();

    Summary {
        nodes_total,
        nodes_ready,
        namespaces: namespaces_count,
        pods_total: pods.len(),
        pods_running,
        pods_pending,
        pods_failed,
        pods_succeeded,
        deployments_total,
        deployments_ready,
        services: services_count,
        events_recent,
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
        assert_eq!(v.summary.pods_running, 1);
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
            pods_running: 3,
            pods_failed: 0,
            events_recent: 1,
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
            if line.contains("pods running") {
                row_text = line;
                // The label cells should carry the muted_fg color.
                for x in 0..buf.area().width {
                    let cell = &buf[(x, y)];
                    if cell.symbol() == "p" {
                        row_fg = cell.style().fg;
                        break;
                    }
                }
                break;
            }
        }
        assert!(
            !row_text.is_empty(),
            "expected to find a 'pods running' trend row"
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
        assert_eq!(v.summary.pods_running, 2);
        assert_eq!(v.summary.pods_pending, 1);
        assert_eq!(v.summary.pods_failed, 1);
        assert_eq!(v.summary.pods_total, 4);
    }
}
