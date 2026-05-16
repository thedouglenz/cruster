# cruster — landing page

Plain HTML + CSS. No build step. Two files (`index.html`,
`styles.css`) plus this README.

## Local preview

```sh
cd web
python3 -m http.server 8000
# open http://localhost:8000/
```

Any static server works — `npx serve`, `caddy file-server`, etc.

## Deploy

The whole directory is static. Drop it on any host:

### GitHub Pages

In repo Settings → Pages, set the source to `main` branch and
`/web` as the folder. The page goes live at
`https://thedouglenz.github.io/cruster/`.

### Netlify / Vercel

Point the project at this directory and set the publish path to
`web/`. No build command needed.

### S3 / Cloudfront / nginx

`cp -R web/* /var/www/cruster/` (or `aws s3 sync web/ s3://bucket/`).

## Editing

- `index.html` is the only page. Sections are top-to-bottom in
  source order: hero, pillars, comparison, benchmarks, agent
  matrix, install, pricing, footer.
- `styles.css` uses CSS custom properties under `:root` — change
  colours there, not scattered throughout. Mobile-first; two
  breakpoints (640px, 960px).
- No external assets. No web fonts. No JS. Keep it that way unless
  there's a real reason to add weight.

## Updating benchmark numbers

The benchmark stats block currently shows design targets from the
spec. Once the benchmark harness produces measured numbers, swap
them in here and remove the "design targets" caveat.
