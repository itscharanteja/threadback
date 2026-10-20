import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const directory = path.dirname(fileURLToPath(import.meta.url));
const icons = [
  {
    id: '01-return-loop', name: 'Return Loop', background: '#173d2d', foreground: '#f8f7eb', accent: '#86d4a0',
    art: (fg, ac) => `
      <path d="M332 152c-24-21-55-33-90-33-77 0-137 61-137 137s60 137 137 137c59 0 108-37 127-88" fill="none" stroke="${fg}" stroke-width="38" stroke-linecap="round"/>
      <path d="M301 146h57v57" fill="none" stroke="${ac}" stroke-width="38" stroke-linecap="round" stroke-linejoin="round"/>
      <circle cx="243" cy="256" r="27" fill="${ac}"/>`,
  },
  {
    id: '02-thread-path', name: 'Thread Path', background: '#f3f0e4', foreground: '#285c46', accent: '#e7a65e',
    art: (fg, ac) => `
      <path d="M150 142c100 0 45 92 147 92 104 0 101 118-11 118h-30c-38 0-61 17-61 39" fill="none" stroke="${fg}" stroke-width="34" stroke-linecap="round"/>
      <circle cx="150" cy="142" r="25" fill="${ac}"/>
      <circle cx="297" cy="234" r="20" fill="${fg}"/>
      <circle cx="195" cy="391" r="26" fill="${ac}"/>`,
  },
  {
    id: '03-checkpoint', name: 'Checkpoint', background: '#174b4b', foreground: '#f9f7eb', accent: '#e9c071',
    art: (fg, ac) => `
      <path d="M158 121h196a18 18 0 0 1 18 18v245l-116-70-116 70V139a18 18 0 0 1 18-18z" fill="none" stroke="${fg}" stroke-width="28" stroke-linejoin="round"/>
      <circle cx="256" cy="221" r="51" fill="${ac}"/>
      <path d="m230 220 18 19 36-41" fill="none" stroke="#174b4b" stroke-width="16" stroke-linecap="round" stroke-linejoin="round"/>`,
  },
  {
    id: '04-evidence-list', name: 'Evidence List', background: '#121b17', foreground: '#f8f7ee', accent: '#70d89a',
    art: (fg, ac) => `
      <rect x="124" y="137" width="264" height="45" rx="14" fill="${fg}"/>
      <rect x="124" y="234" width="184" height="45" rx="14" fill="${fg}"/>
      <path d="m327 231 61 25-61 25z" fill="${ac}"/>
      <rect x="124" y="331" width="264" height="45" rx="14" fill="${fg}"/>`,
  },
  {
    id: '05-waypoint', name: 'Waypoint', background: '#244966', foreground: '#f8f9f2', accent: '#7ed5df',
    art: (fg, ac) => `
      <path d="M153 355V239c0-60 40-101 103-101h103" fill="none" stroke="${fg}" stroke-width="35" stroke-linecap="round"/>
      <circle cx="153" cy="355" r="31" fill="${ac}"/>
      <circle cx="359" cy="138" r="31" fill="${ac}"/>
      <path d="M220 269h133m-54-53 54 53-54 53" fill="none" stroke="${fg}" stroke-width="32" stroke-linecap="round" stroke-linejoin="round"/>`,
  },
  {
    id: '06-resume-capsule', name: 'Resume Capsule', background: '#4b405d', foreground: '#fff8ef', accent: '#e5b9d1',
    art: (fg, ac) => `
      <rect x="112" y="163" width="288" height="186" rx="93" fill="none" stroke="${fg}" stroke-width="29"/>
      <path d="M257 174v164" fill="none" stroke="${fg}" stroke-width="28"/>
      <circle cx="175" cy="256" r="13" fill="${ac}"/><circle cx="215" cy="256" r="13" fill="${ac}"/>
      <path d="M298 256h58m-24-24 24 24-24 24" fill="none" stroke="${ac}" stroke-width="21" stroke-linecap="round" stroke-linejoin="round"/>`,
  },
  {
    id: '07-branch-back', name: 'Branch Back', background: '#dcece3', foreground: '#205d46', accent: '#d9805e',
    art: (fg, ac) => `
      <path d="M154 132v204c0 39 25 59 60 59h134" fill="none" stroke="${fg}" stroke-width="31" stroke-linecap="round"/>
      <path d="M154 247c0-45 32-69 74-69h114" fill="none" stroke="${fg}" stroke-width="31" stroke-linecap="round"/>
      <circle cx="154" cy="132" r="27" fill="${ac}"/><circle cx="342" cy="178" r="27" fill="${ac}"/>
      <path d="m318 365 33 30-33 30" fill="none" stroke="${fg}" stroke-width="29" stroke-linecap="round" stroke-linejoin="round"/>`,
  },
  {
    id: '08-folded-note', name: 'Folded Note', background: '#a45b45', foreground: '#fff8e9', accent: '#f0c673',
    art: (fg, ac) => `
      <path d="M146 119h164l66 66v208H146c-15 0-27-12-27-27V146c0-15 12-27 27-27z" fill="none" stroke="${fg}" stroke-width="28" stroke-linejoin="round"/>
      <path d="M308 120v67h67" fill="none" stroke="${ac}" stroke-width="25" stroke-linejoin="round"/>
      <path d="M176 249h120m-120 57h86" fill="none" stroke="${fg}" stroke-width="25" stroke-linecap="round"/>
      <circle cx="338" cy="315" r="32" fill="${ac}"/>`,
  },
  {
    id: '09-tb-monogram', name: 'TB Monogram', background: '#3d4d32', foreground: '#fff9df', accent: '#d9e994',
    art: (fg, ac) => `
      <path d="M124 151h133m-67 0v218" fill="none" stroke="${fg}" stroke-width="37" stroke-linecap="round"/>
      <path d="M278 149v221h43c53 0 78-27 78-64 0-28-19-46-50-54 27-10 42-29 42-52 0-30-21-51-67-51h-46z" fill="none" stroke="${ac}" stroke-width="31" stroke-linecap="round" stroke-linejoin="round"/>
      <path d="M284 253h59" fill="none" stroke="${ac}" stroke-width="25" stroke-linecap="round"/>`,
  },
  {
    id: '10-next-compass', name: 'Next Compass', background: '#252c4a', foreground: '#f4f7f4', accent: '#9fcbeb',
    art: (fg, ac) => `
      <circle cx="256" cy="256" r="136" fill="none" stroke="${fg}" stroke-width="28"/>
      <path d="m332 168-53 121-111 54 52-119z" fill="${ac}"/>
      <circle cx="256" cy="256" r="25" fill="#252c4a"/>
      <circle cx="256" cy="120" r="11" fill="${ac}"/>`,
  },
];

for (const icon of icons) {
  const topHighlight = icon.id === '04-evidence-list'
    ? ''
    : '<path d="M76 56c98-32 249-32 351 29" fill="none" stroke="#fff" stroke-opacity=".08" stroke-width="36" stroke-linecap="round"/>';
  const app = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" role="img" aria-label="${icon.name} app icon">
  <rect x="20" y="20" width="472" height="472" rx="111" fill="${icon.background}"/>
  ${topHighlight}
  ${icon.art(icon.foreground, icon.accent)}
</svg>\n`;
  const menu = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="82 82 348 348" role="img" aria-label="${icon.name} menu bar glyph">
  ${icon.art('#26372c', '#26372c')}
</svg>\n`;
  fs.writeFileSync(path.join(directory, `${icon.id}.svg`), app.replace(/[ \t]+$/gm, ''));
  fs.writeFileSync(path.join(directory, `${icon.id}-menubar.svg`), menu.replace(/[ \t]+$/gm, ''));
}
console.log(`Generated ${icons.length} app icons and ${icons.length} menu bar glyphs.`);
