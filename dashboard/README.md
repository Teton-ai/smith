# Smith dashboard

Fully client-side React SPA for the Smith fleet-management system. Built with
**Vite + React 19**, routed with **react-router**, styled with **Tailwind CSS v4**.
No Next.js, no SSR, no server components, no API routes.

## Getting Started

From the repo root (npm workspaces) or from this directory:

```bash
npm install        # run once at the repo root
npm run dev        # start the Vite dev server
```

Open [http://localhost:3000](http://localhost:3000) with your browser.

## Structure

- `src/main.tsx` — app entry point.
- `src/router.tsx` — all routes, wired up manually with `createBrowserRouter`.
- `app/` — page/layout components. Files keep the `page.tsx` / `layout.tsx`
  naming and `(private)` route-group folders, but routing is **not** file-based:
  add a page by creating the component and registering it in `src/router.tsx`.
- Shared design-system primitives (buttons, cards, badges, …) come from the
  `@teton/smith-ui` workspace package (`packages/ui`), not from this app. Import
  them with `import { Button, Card } from "@teton/smith-ui"`.

## Scripts

- `npm run dev` — Vite dev server.
- `npm run build` — production build.
- `npm run preview` — preview the production build.
- `npm run lint` — Biome format/lint.
- `npm run gen-api-client` — regenerate the API client with Orval.
