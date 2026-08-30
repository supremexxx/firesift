# FireSift web

React + TypeScript + Tailwind CSS v4, built with Vite. This is the
interface layer for FireSift, independent of the Rust workspace at the
repository root — its own `package.json`, its own build, never mixed
with Cargo.

As of this writing this is scaffolding only: the design tokens in
`src/index.css` (`@theme` block) and a placeholder page in `src/App.tsx`
prove the build pipeline works end to end. The actual public interface
and scientific dashboard are separate, later work — see `ROADMAP.md` at
the repository root.

## Development

```sh
npm install
npm run dev
```

## Build

```sh
npm run build
```

Outputs static assets to `dist/`, which the Rust `api` crate serves at
runtime (see `docs/architecture.md`) — never committed, never served by
a separate Node process in production.

## Lint

```sh
npm run lint
```
