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

## Model connection

ThreadBack asks an NVIDIA Nemotron model on
[Nebius Token Factory](https://tokenfactory.nebius.com) to write the resume
capsule. This is the only network connection it makes, and it happens only when
you press **Generate** after reviewing the exact payload.

1. Create an account at <https://tokenfactory.nebius.com> and an API key. The
   Nebius x NVIDIA hackathon offers Token Factory credits on its Devpost
   resources page.
2. In the app, open **Model**, paste the key, and press **Save**. The key goes
   straight to the macOS Keychain. It is never shown again, never written to
   ThreadBack's database, and never exported.
3. Press **Test connection**. It sends one tiny question that contains none of
   your evidence.

Defaults: endpoint `https://api.tokenfactory.nebius.com/v1`, model
`nvidia/nemotron-3-super-120b-a12b`, 60 second timeout, strict JSON-schema
output (ThreadBack steps down to a simpler format on its own if the service
rejects it). The endpoint must use `https`; plain `http` is accepted only for a
loopback address, so a local server can stand in during development.

The `tb` command has no network code at all: it is built without the model
client and the Keychain adapter.

## Privacy boundary

ThreadBack must not capture screenshots, keystrokes, global shell history, or
files outside the approved canonical project root. No model request may occur
until the user reviews and confirms the exact redacted payload.
