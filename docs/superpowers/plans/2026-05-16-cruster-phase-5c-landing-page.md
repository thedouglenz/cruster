# Cruster Phase 5C: Landing page

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a static landing page at `web/` that explains
cruster to a visitor in under a minute, lists install options,
shows benchmark targets + agent compatibility, and surfaces pricing.

**Scope guardrail:** Plain HTML + CSS, no build step, no framework.
The site is a few static files anyone can serve from GitHub Pages,
Netlify, Vercel, S3, or `python3 -m http.server`. No JavaScript
unless absolutely necessary.

**Out of scope:**
- A blog or docs site (separate concern; the spec doesn't mention one).
- Auth / signup / payment flows (no backend; pricing card just links
  to `mailto:dev@cruster.dev` for v1).
- Live cluster demo / playground (too speculative).
- Real-time benchmark numbers — use the spec's design targets
  labelled as such, with a footnote that they're targets, not yet-
  measured production data.

## Tech choice

Plain `index.html` + `styles.css`. CSS variables for the colour
scheme so it can swap themes (matches the product). Terminal
aesthetic: monospaced headlines, muted-background palette, ASCII
flourishes where they read well.

Why not Astro/Next/etc.: a single landing page doesn't need a
framework. Plain HTML deploys anywhere, has zero JS to ship, never
breaks on dependency churn, and a future contributor doesn't need
to learn the toolchain.

## Sections (in order)

1. **Hero**: tagline ("Kubernetes TUI built for the agent era"),
   one-line value prop, primary install command.
2. **Three pillars**: speed, ergonomics, agent compatibility. Three
   columns pulled from the spec.
3. **Agent-first numbers**: side-by-side comparison of the same
   diagnosis task done via `kubectl` vs `cruster export`.
4. **Benchmark targets**: cold start, refresh latency, memory
   footprint on a 10k-pod cluster.
5. **Agent compatibility matrix**: the agents agentskills.io ships
   with, marked installed/supported.
6. **Install**: cargo, brew, prebuilt binary, from source.
7. **Pricing**: free / pro / team / enterprise. Pro and team have
   a "contact" link (mailto:) for v1 since there's no payment flow.
8. **Footer**: repo link, license, contact email.

---

## Task 1: Page skeleton + styles

- [ ] Create `web/index.html` with semantic sections matching the
  list above. No JS.
- [ ] Create `web/styles.css` with:
  - CSS custom properties for colors (terminal-aesthetic palette).
  - A mobile-first layout: single column on phones, two-column for
    the pillars + comparison sections on wider viewports.
  - System monospace stack for headlines + code; system sans stack
    for body. No web font fetches.
- [ ] Hero, three pillars, install section. Commit when these render.

## Task 2: Numbers + compatibility matrix sections

- [ ] Agent-first numbers section: a two-column ASCII-style table.
  Left column = `kubectl describe pod && kubectl logs && kubectl
  get events`. Right column = `cruster export pod/foo`. Counts of
  calls, bytes, wall-clock. Footnote: measured on the staged
  failing-pod scenario in agent-platform on a k3d cluster.
- [ ] Benchmark targets section: three stat blocks. Cold start
  target, 10k-pod scrolling CPU target, 10k-pod memory target.
  Label them clearly as design targets.
- [ ] Agent compatibility matrix: table with Claude Code, Cursor,
  Codex, Gemini CLI, Goose, OpenHands. Note the installation path
  per agent, since they vary.
- [ ] Commit.

## Task 3: Pricing + footer

- [ ] Four-card pricing grid: Free, Pro, Team, Enterprise. Match
  the figures in the spec (`$0`, `$99/yr`, `$29/mo/user`, `Contact`).
- [ ] Each card lists the headline features for that tier.
- [ ] Pro/Team CTAs are `mailto:dev@cruster.dev` for v1 (no payment
  flow). Free CTA links to the install section. Enterprise CTA is
  also mailto.
- [ ] Footer: GitHub repo link, licence note (Proprietary), contact.
- [ ] Commit.

## Task 4: web/README + deploy docs

- [ ] `web/README.md` covering:
  - Local preview: `cd web && python3 -m http.server 8000`.
  - Deploy: drop `web/` onto any static host. Examples for GitHub
    Pages (just point the Pages config at `/web`) and Netlify
    (drop-in directory).
- [ ] Top-level README mentions the landing page lives in `web/`
  and link to it (no live URL yet — that's a deploy-time thing).
- [ ] Bump README status to Phase 5C / project complete.
- [ ] Commit.

## Task 5: Verify + tag

- [ ] HTML validates (no missing closing tags / broken refs):
  `python3 -c "import html.parser; html.parser.HTMLParser().feed(open('web/index.html').read()); print('ok')"`
  (a real validator would be better but stdlib is enough to catch
  syntax issues).
- [ ] CSS file parses: `python3 -c "import tinycss2; tinycss2.parse_stylesheet(open('web/styles.css').read())"`
  — if tinycss2 isn't installed, skip; visual review is the fallback.
- [ ] Serve locally and smoke: `cd web && python3 -m http.server 8765 &`,
  curl `http://localhost:8765/` and confirm 200 + content-length > 0.
- [ ] Tag `phase-5c-landing-page`.

When 5C ships, the v1 MVP scope is closed.
