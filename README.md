# ThreadBack

ThreadBack is a private interruption-recovery assistant for software debugging.
It preserves evidence and reasoning state inside one approved Git project, then
produces a source-backed resume capsule for the user to review and approve.

## Status

The project is under active MVP development. See `TODO.md` for the executable
backlog and `spec.md` for the product contract.

## Prerequisites

- macOS
- Node.js 22.12 or newer
- npm 10 or newer
- Rust stable
- Xcode Command Line Tools

## Development

```bash
npm install
npm run tauri dev
```

Run validation with:

```bash
npm run check
npm test
npm run format
```

## Privacy boundary

ThreadBack must not capture screenshots, keystrokes, global shell history, or
files outside the approved canonical project root. No model request may occur
until the user reviews and confirms the exact redacted payload.
