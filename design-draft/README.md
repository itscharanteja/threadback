# ThreadBack UI draft

Open `index.html` to explore 11 screens using the draft-only screen selector or the product buttons. Sample data only; no backend requests or real file access. Connection, folder selection, and settings are simulated. Summary editing is a visual interaction sketch.

## Direction

Soft off-white surfaces, restrained forest-green actions, Inter/system typography, generous spacing, and everyday language. One main action per stage; deeper evidence and configuration are disclosed on demand.

Flow: add approved Git project → start work → record explicit commands and notes → save locally → optionally review AI sharing → inspect/edit AI draft → approve → check project changes → resume.

Rename payload to “what AI receives”, capsule to “summary”, evidence to “activity”, hypothesis to “idea”, and repository drift to “your project changed”. The product still supports debugging in Git projects; accessible language does not imply support for arbitrary non-code folders.

## Source grounding

Reviewed README, personas, frontend screens and components, global styles, lifecycle rules, and resume comparison. Preserve explicit collection, local save, blocked-credential exclusion, separate approval, source links, project-boundary controls, retention exceptions, and commands never running automatically.

This is a proposed light visual direction. Existing app styling is dark; production styling has not been changed.

## Figma

Draft file: https://www.figma.com/design/RHAMQuHkjl46NLRU1eM3uX

Figma Starter MCP quota prevented canvas composition. The file is blank. Import the SVG files in `figma-import/` into Figma for editable vector and text screen references. PNG files are visual previews only. SVG text is split into words and does not yet have Figma auto-layout, component instances, token bindings, or prototype connections. A native component-based Figma build remains pending tool availability.

## Verification

All 11 screens render. Local consent, secret exclusion, and note interactions checked with jsdom. Key layouts checked in Chromium at 1440, 736, and 390 pixels for horizontal overflow. Desktop home and sharing review previews visually inspected.

`export.cjs` regenerates SVG and PNG references using the bundled Playwright runtime and installed Chrome. No production source files were edited for this draft.
