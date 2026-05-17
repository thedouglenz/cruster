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
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph, Sparkline};
use ratatui::Frame;

use crate::app::LoopState;
use crate::dashboard::{DashboardConfig, Pin};
use crate::overlays::search::Filter;
use crate::view::ResourceView;

const HISTORY_CAP: usize = 60;
const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

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

    fn pin_history(&self, pin_idx: usize) -> Vec<u64> {
        self.history
            .iter()
            .map(|s| s.pin_values.get(pin_idx).copied().unwrap_or(0))
            .collect()
    }

    fn pins_section_lines(&self) -> u16 {
        // Two render lines per pin (label + gauge/status), at least
        // one line for the "no pins" hint.
        if self.config.pins.is_empty() {
            return 1;
        }
        (self.config.pins.len() as u16).saturating_mul(2)
    }
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

    fn render(&self, frame: &mut Frame<'_>) {
        let area = frame.area();

        let pins_h = self.pins_section_lines().max(2) + 1; // +1 for border
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4), // summary band (top border + 2 lines + 1 padding)
                Constraint::Min(6),    // trends band fills remainder
                Constraint::Length(pins_h),
            ])
            .split(area);

        self.render_summary(frame, chunks[0]);
        self.render_trends(frame, chunks[1]);
        self.render_pins(frame, chunks[2]);
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

    fn render_trends(&self, frame: &mut Frame<'_>, area: Rect) {
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

        draw_trend_row(frame, rows[0], "pods running", &pods_running, Color::Green);
        draw_trend_row(frame, rows[1], "pods failed ", &pods_failed, Color::Red);
        draw_trend_row(
            frame,
            rows[2],
            "events      ",
            &events_recent,
            Color::Yellow,
        );
    }

    fn render_pins(&self, frame: &mut Frame<'_>, area: Rect) {
        let block = Block::default()
            .borders(Borders::TOP)
            .title(" pinned · [a] add from any view · [x] unpin · [enter] open ");
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.config.pins.is_empty() {
            let hint = Paragraph::new(
                "no pins yet — switch to a list view (`:pods`, `:deploy`, …) and press `a`",
            )
            .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(hint, inner);
            return;
        }

        if inner.height == 0 {
            return;
        }

        // Two rows per pin: label/marker + gauge/status. If we run out
        // of vertical space, render whatever fits.
        let mut y = inner.y;
        for (i, pin) in self.config.pins.iter().enumerate() {
            if y + 1 >= inner.y + inner.height {
                break;
            }
            let snap = self.pin_snapshots.get(i);
            let is_selected = i == self.selected;
            let marker = if is_selected { "▎" } else { " " };
            let label = format!("{marker} {}", pin.label());
            let style = if is_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let label_para = Paragraph::new(label).style(style);
            let label_rect = Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: 1,
            };
            frame.render_widget(label_para, label_rect);

            let detail_rect = Rect {
                x: inner.x + 2,
                y: y + 1,
                width: inner.width.saturating_sub(2),
                height: 1,
            };
            if detail_rect.y < inner.y + inner.height {
                render_pin_detail(frame, detail_rect, pin, snap, &self.pin_history(i));
            }
            y += 2;
        }
    }
}

fn draw_trend_row(frame: &mut Frame<'_>, area: Rect, label: &str, data: &[u64], color: Color) {
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
        Paragraph::new(label.to_string()).style(Style::default().fg(Color::DarkGray)),
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

fn render_pin_detail(
    frame: &mut Frame<'_>,
    area: Rect,
    pin: &Pin,
    snap: Option<&PinSnapshot>,
    history: &[u64],
) {
    let Some(snap) = snap else {
        frame.render_widget(
            Paragraph::new("…").style(Style::default().fg(Color::DarkGray)),
            area,
        );
        return;
    };
    if snap.looked_up
        && snap.pod.is_none()
        && snap.deployment.is_none()
        && snap.service.is_none()
        && snap.node.is_none()
    {
        frame.render_widget(
            Paragraph::new("(missing)").style(Style::default().fg(Color::Red)),
            area,
        );
        return;
    }

    match pin.kind.as_str() {
        "Deployment" => render_deployment_detail(frame, area, snap.deployment.as_ref(), history),
        "Pod" => render_pod_detail(frame, area, snap.pod.as_ref(), history),
        "Service" => render_service_detail(frame, area, snap.service.as_ref()),
        "Node" => render_node_detail(frame, area, snap.node.as_ref(), history),
        other => {
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(other.to_string(), Style::default().fg(Color::DarkGray)),
                    Span::raw("  pinned"),
                ])),
                area,
            );
        }
    }
}

fn render_deployment_detail(
    frame: &mut Frame<'_>,
    area: Rect,
    deploy: Option<&Deployment>,
    history: &[u64],
) {
    let Some(d) = deploy else {
        frame.render_widget(Paragraph::new("…"), area);
        return;
    };
    let desired = d.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0).max(0) as u16;
    let ready = d
        .status
        .as_ref()
        .and_then(|s| s.ready_replicas)
        .unwrap_or(0)
        .max(0) as u16;
    let ratio = if desired == 0 {
        0.0
    } else {
        (ready as f64 / desired as f64).clamp(0.0, 1.0)
    };

    // Layout: gauge | text | sparkline
    let gauge_w = area.width.min(20);
    let text = format!("  {ready}/{desired}");
    let text_w = text.len() as u16;
    let spark_w = area.width.saturating_sub(gauge_w + text_w + 1);

    let gauge_rect = Rect {
        x: area.x,
        y: area.y,
        width: gauge_w,
        height: 1,
    };
    let text_rect = Rect {
        x: gauge_rect.x + gauge_rect.width,
        y: area.y,
        width: text_w,
        height: 1,
    };
    let spark_rect = Rect {
        x: text_rect.x + text_rect.width + 1,
        y: area.y,
        width: spark_w,
        height: 1,
    };

    let gauge = Gauge::default()
        .ratio(ratio)
        .gauge_style(Style::default().fg(if ratio >= 1.0 {
            Color::Green
        } else {
            Color::Yellow
        }))
        .label(format!("{:>3.0}%", ratio * 100.0));
    frame.render_widget(gauge, gauge_rect);
    frame.render_widget(Paragraph::new(text), text_rect);
    if spark_w >= 4 {
        let spark = Sparkline::default()
            .data(history)
            .style(Style::default().fg(Color::Green));
        frame.render_widget(spark, spark_rect);
    }
}

fn render_pod_detail(frame: &mut Frame<'_>, area: Rect, pod: Option<&Pod>, history: &[u64]) {
    let Some(p) = pod else {
        frame.render_widget(Paragraph::new("…"), area);
        return;
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
    let phase_color = match phase.as_str() {
        "Running" => Color::Green,
        "Pending" => Color::Yellow,
        "Failed" => Color::Red,
        "Succeeded" => Color::Blue,
        _ => Color::DarkGray,
    };
    let text = format!("{phase}  ready {ready}/{total}  restarts {restarts}");
    let label_w = (text.len() as u16).min(area.width);
    let label_rect = Rect {
        x: area.x,
        y: area.y,
        width: label_w,
        height: 1,
    };
    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(phase_color)),
        label_rect,
    );
    let spark_w = area.width.saturating_sub(label_w + 1);
    if spark_w >= 4 {
        let spark_rect = Rect {
            x: label_rect.x + label_rect.width + 1,
            y: area.y,
            width: spark_w,
            height: 1,
        };
        let spark = Sparkline::default()
            .data(history)
            .style(Style::default().fg(Color::Magenta));
        frame.render_widget(spark, spark_rect);
    }
}

fn render_service_detail(frame: &mut Frame<'_>, area: Rect, svc: Option<&Service>) {
    let Some(s) = svc else {
        frame.render_widget(Paragraph::new("…"), area);
        return;
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
    let text = format!("{svc_type}  {cluster_ip}  {ports} port(s)");
    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(Color::Cyan)),
        area,
    );
}

fn render_node_detail(frame: &mut Frame<'_>, area: Rect, node: Option<&Node>, history: &[u64]) {
    let Some(n) = node else {
        frame.render_widget(Paragraph::new("…"), area);
        return;
    };
    let ready = node_is_ready(n);
    let (status, color) = if ready {
        ("Ready", Color::Green)
    } else {
        ("NotReady", Color::Red)
    };
    let pod_count = history.last().copied().unwrap_or(0);
    let text = format!("{status}  pods {pod_count}");
    let label_w = (text.len() as u16).min(area.width);
    let label_rect = Rect {
        x: area.x,
        y: area.y,
        width: label_w,
        height: 1,
    };
    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(color)),
        label_rect,
    );
    let spark_w = area.width.saturating_sub(label_w + 1);
    if spark_w >= 4 {
        let spark_rect = Rect {
            x: label_rect.x + label_rect.width + 1,
            y: area.y,
            width: spark_w,
            height: 1,
        };
        let spark = Sparkline::default()
            .data(history)
            .style(Style::default().fg(color));
        frame.render_widget(spark, spark_rect);
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
