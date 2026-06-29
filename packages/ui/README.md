# @teton/smith-ui

Design-system primitives used by the [Smith](https://github.com/teton-ai/smith)
dashboard: buttons, cards, badges, stat cards, search input, nav, toasts and the
shared theme tokens.

## Install

```bash
npm install @teton/smith-ui
```

Peer dependencies (provide these in your app): `react`, `react-dom`,
`react-router`, `lucide-react`. The components are styled with **Tailwind CSS v4**.

## Usage

```tsx
import { Button, Card, StatCard } from "@teton/smith-ui";
```

### Tailwind setup

The components render plain Tailwind utility classes. For Tailwind to emit those
classes, point its source scanning at this package from your main CSS file:

```css
@import "tailwindcss";
@source "../node_modules/@teton/smith-ui/dist";
```

No separate stylesheet import is required — the primitives rely only on standard
Tailwind utilities.

## License

Apache-2.0
