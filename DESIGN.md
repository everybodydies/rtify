---
version: alpha
name: system24 OLED
description: >
  TUI-inspiriertes Dark-UI nach dem Vorbild des Discord-Themes "system24" von refact0r,
  optimiert für OLED-Displays: echtes Schwarz als Fläche, Struktur ausschließlich über Rahmen,
  gedimmte Texthelligkeit gegen Blooming und Burn-in. Monospace-Schrift, eckige Panels mit
  Rahmen-Labels, achromatische Graustufen in OKLCH und ein einzelner Pastell-Akzent in Mint.

colors:
  # Rolle
  primary: "oklch(83% 0.06 182)"          # = accent-2, #9BD4C9
  neutral: "oklch(0% 0 0)"                # = bg-4,     #000000

  # Text (hell → dunkel) – bewusst unter Reinweiß
  text-0: "oklch(0% 0 0)"                 # Text auf farbigen Flächen, #000000
  text-1: "oklch(96% 0 0)"                # hellster Text, Auswahl, #F0F0F0
  text-2: "oklch(92% 0 0)"                # Überschriften, wichtiger Text, #E6E6E6
  text-3: "oklch(71% 0 0)"                # Fließtext, #A0A0A0
  text-4: "oklch(63% 0 0)"                # Icons, Listeneinträge, Labels, #8A8A8A
  text-5: "oklch(42% 0 0)"                # nur dekorativ: Platzhalter, #4D4D4D

  # Hintergründe
  bg-1: "oklch(24% 0 0)"                  # Buttons gedrückt/hover, #1F1F1F
  bg-2: "oklch(19% 0 0)"                  # Buttons, #141414
  bg-3: "oklch(0% 0 0)"                   # Basisebene / Gaps / Popouts, #000000
  bg-4: "oklch(0% 0 0)"                   # Panel-Fläche, #000000

  # Zustände – deckend, nie dunkler als L 18 % (Anti-Smearing)
  hover: "oklch(19% 0 0)"                 # ≈ #141414
  active: "oklch(24% 0 0)"                # ≈ #1F1F1F
  active-2: "oklch(29% 0 0)"              # ≈ #2A2A2A

  # Rahmen – tragen auf OLED die gesamte Struktur
  border-light: "oklch(19% 0 0)"          # interne Trennlinien, ≈ #141414
  border: "oklch(29% 0 0)"                # Panel-Rahmen, ≈ #2A2A2A
  border-hover: "oklch(83% 0.06 182)"     # = accent-2
  button-border: "oklch(29% 0 0)"         # #2A2A2A

  # Akzent (Mint, Hue 182)
  accent-1: "oklch(88% 0.06 182)"         # Links, Akzenttext hell, #B5E2D9
  accent-2: "oklch(83% 0.06 182)"         # Signalton: Panel-Labels, Fokus, Hover, #9BD4C9
  accent-3: "oklch(78% 0.06 182)"         # Akzent-Buttons, #87C4B9
  accent-4: "oklch(73% 0.06 182)"         # Akzent-Buttons Hover, #73B4A9
  accent-5: "oklch(68% 0.06 182)"         # nur dekorativ, #5FA499
  accent-new: "oklch(78% 0.09 19)"        # "Neu"/Warn-Akzent, Validierung, #E8A0A0

  # Status
  online: "oklch(70% 0.12 170)"           # green-2, #3BB693
  idle: "oklch(75% 0.12 90)"              # yellow-2, #CBAA4B
  dnd: "oklch(78% 0.09 19)"               # accent-new, #E8A0A0
  streaming: "oklch(83% 0.06 182)"        # accent-2, #9BD4C9
  offline: "oklch(63% 0 0)"               # text-4, #8A8A8A

  # Basispalette (gleiche Helligkeitsstufen, nur Hue wechselt)
  red-1: "oklch(75% 0.13 0)"
  red-2: "oklch(70% 0.13 0)"
  red-3: "oklch(65% 0.13 0)"
  red-4: "oklch(60% 0.13 0)"
  red-5: "oklch(55% 0.13 0)"
  green-1: "oklch(75% 0.12 170)"
  green-2: "oklch(70% 0.12 170)"
  green-3: "oklch(65% 0.12 170)"
  green-4: "oklch(60% 0.12 170)"
  green-5: "oklch(55% 0.12 160)"
  blue-1: "oklch(75% 0.11 215)"
  blue-2: "oklch(70% 0.11 215)"
  blue-3: "oklch(65% 0.11 215)"
  blue-4: "oklch(60% 0.11 215)"
  blue-5: "oklch(55% 0.11 215)"
  yellow-1: "oklch(80% 0.12 90)"
  yellow-2: "oklch(75% 0.12 90)"
  yellow-3: "oklch(70% 0.12 90)"
  yellow-4: "oklch(65% 0.12 90)"
  yellow-5: "oklch(60% 0.12 90)"
  purple-1: "oklch(75% 0.12 310)"
  purple-2: "oklch(70% 0.12 310)"
  purple-3: "oklch(65% 0.12 310)"
  purple-4: "oklch(60% 0.12 310)"
  purple-5: "oklch(55% 0.12 310)"

typography:
  heading-lg:
    fontFamily: DM Mono
    fontSize: 1.25rem
    fontWeight: 500
    lineHeight: 1.3
    letterSpacing: -0.03em
  heading-sm:
    fontFamily: DM Mono
    fontSize: 1rem
    fontWeight: 500
    lineHeight: 1.3
    letterSpacing: -0.03em
  body-md:
    fontFamily: DM Mono
    fontSize: 1rem
    fontWeight: 400
    lineHeight: 1.4
    letterSpacing: -0.03em
  body-sm:
    fontFamily: DM Mono
    fontSize: 0.875rem
    fontWeight: 400
    lineHeight: 1.35
    letterSpacing: -0.03em
  label:
    fontFamily: DM Mono
    fontSize: 0.75rem
    fontWeight: 500
    lineHeight: 1
    letterSpacing: -0.03em
  meta:
    fontFamily: DM Mono
    fontSize: 0.75rem
    fontWeight: 400
    lineHeight: 1.3
    letterSpacing: -0.03em
  code:
    fontFamily: DM Mono
    fontSize: 0.875rem
    fontWeight: 400
    lineHeight: 1.4
    letterSpacing: 0em

rounded:
  none: 0px

spacing:
  xs: 4px
  sm: 8px
  gap: 12px
  lg: 16px
  xl: 24px
  border: 1px
  divider: 4px

components:
  app-base:
    backgroundColor: "{colors.bg-3}"
  panel:
    backgroundColor: "{colors.bg-4}"
    textColor: "{colors.text-3}"
    rounded: "{rounded.none}"
    padding: 8px
  panel-label:
    backgroundColor: "{colors.bg-4}"
    textColor: "{colors.text-4}"
    typography: "{typography.label}"
    padding: 0 4px
  list-item:
    textColor: "{colors.text-4}"
    typography: "{typography.body-md}"
    rounded: "{rounded.none}"
    padding: 4px 8px
  list-item-hover:
    backgroundColor: "{colors.hover}"
    textColor: "{colors.text-3}"
  list-item-active:
    backgroundColor: "{colors.active}"
    textColor: "{colors.text-1}"
  list-item-muted:
    textColor: "{colors.text-5}"
  button:
    backgroundColor: "{colors.bg-2}"
    textColor: "{colors.text-2}"
    typography: "{typography.body-sm}"
    rounded: "{rounded.none}"
    padding: 6px 12px
  button-hover:
    backgroundColor: "{colors.bg-1}"
  button-ghost:
    backgroundColor: "{colors.bg-4}"
    textColor: "{colors.text-2}"
    rounded: "{rounded.none}"
    padding: 6px 12px
  button-accent:
    backgroundColor: "{colors.accent-3}"
    textColor: "{colors.text-0}"
    rounded: "{rounded.none}"
    padding: 6px 12px
  button-accent-hover:
    backgroundColor: "{colors.accent-4}"
    textColor: "{colors.text-0}"
  button-accent-pressed:
    backgroundColor: "{colors.accent-4}"
    textColor: "{colors.text-0}"
  button-danger:
    backgroundColor: "{colors.accent-new}"
    textColor: "{colors.text-0}"
    rounded: "{rounded.none}"
  input:
    backgroundColor: "{colors.bg-4}"
    textColor: "{colors.text-2}"
    typography: "{typography.body-md}"
    rounded: "{rounded.none}"
    padding: 8px 12px
  popout:
    backgroundColor: "{colors.bg-3}"
    textColor: "{colors.text-3}"
    rounded: "{rounded.none}"
    padding: 8px
  link:
    textColor: "{colors.accent-1}"
  timestamp:
    textColor: "{colors.text-5}"
    typography: "{typography.meta}"
  unread-divider:
    backgroundColor: "{colors.accent-new}"
    height: 4px
  status-online:
    backgroundColor: "{colors.online}"
    size: 8px
  status-idle:
    backgroundColor: "{colors.idle}"
    size: 8px
  status-dnd:
    backgroundColor: "{colors.dnd}"
    size: 8px
  status-streaming:
    backgroundColor: "{colors.streaming}"
    size: 8px
  status-offline:
    backgroundColor: "{colors.offline}"
    size: 8px
  avatar:
    rounded: "{rounded.none}"
    size: 32px
---

## Overview

**Terminal-Ästhetik auf echtem Schwarz.** system24 OLED übersetzt die Formensprache einer TUI
(htop, btop, lazygit, ncurses-Dialoge) in ein modernes, mausbedienbares Interface und ist
auf OLED-Displays zugeschnitten. Die Oberfläche besteht aus klar abgegrenzten, rechteckigen
**Panels** auf `#000000`. Jedes Panel hat einen dünnen Rahmen und ein kleines
**Label im oberen Rahmen**, wie ein `┌─ channels ──────┐` in einem Terminal-Fenster.

Charakter in drei Worten: **ruhig, technisch, präzise.**

- **Echtes Schwarz** für Panels und Gaps: Die Pixel sind aus, das spart Energie und ergibt
  unendlichen Kontrast. Struktur entsteht **nur über Linien**, nicht über Flächen.
- Alles ist **monospace** (DM Mono).
- **Keine Rundungen**, keine Schatten, keine Verläufe als Flächenfüllung.
- Farbe ist fast vollständig **achromatisch**; ein einziger gedämpfter Pastell-Akzent
  (Mint) markiert Interaktion und Fokus.
- Verspielte Details kommen aus der Terminal-Welt: ASCII-Titel, ASCII-Ladeanimation,
  Fortschrittsbalken aus Textzeichen.

Das Original ist ein Discord-Theme; die Regeln hier gelten für jede App mit Panel-Layout
(Chat, Dashboards, Dev-Tools, Dateimanager, Musikplayer).

## Colors

Die Palette ist in **OKLCH** definiert. Alle Grautöne haben Chroma 0, und alle Akzentfarben
liegen auf **identischen Helligkeitsstufen** (88 / 83 / 78 / 73 / 68 %). Der Akzent lässt sich
also durch reinen Hue-Tausch wechseln, ohne dass Kontraste kippen.

### Flächen

| Token | Wert | Hex ≈ | Verwendung |
|:--|:--|:--|:--|
| `bg-4` | `oklch(0% 0 0)` | `#000000` | Panel-Fläche |
| `bg-3` | `oklch(0% 0 0)` | `#000000` | Basisebene, Gaps, Popouts, Menüs |
| `bg-2` | `oklch(19% 0 0)` | `#141414` | neutrale Buttons (kleine Flächen) |
| `bg-1` | `oklch(24% 0 0)` | `#1F1F1F` | Buttons hover/gedrückt |
| `hover` | `oklch(19% 0 0)` | `#141414` | Listeneintrag/Nachricht hover |
| `active` | `oklch(24% 0 0)` | `#1F1F1F` | ausgewählt |
| `active-2` | `oklch(29% 0 0)` | `#2A2A2A` | Zusatzzustand (z. B. gedrückt auf Auswahl) |

Panels und Gaps sind beide schwarz. Was im Original über den Tonwertunterschied
`bg-3`/`bg-4` lief, übernehmen hier die **Rahmen**.

**Untergrenze L 18 %:** Jede Fläche, die nicht schwarz ist, hat mindestens 18 % Helligkeit.
Fast-schwarze Grautöne (`#050505` – `#0F0F0F`) verursachen auf OLED **Black Smearing**
(Schlieren beim Scrollen, weil Pixel aus dem Aus-Zustand verzögert anspringen) und an vielen
Panels **Banding oder einen Farbstich** bei niedriger Helligkeit. Darum sind die Zustände
deckend definiert statt als transparente Overlays.

### Text

| Token | Wert | Hex ≈ | Verwendung |
|:--|:--|:--|:--|
| `text-1` | `oklch(96% 0 0)` | `#F0F0F0` | hellster Text, Titel, ausgewählte Einträge |
| `text-2` | `oklch(92% 0 0)` | `#E6E6E6` | Namen, wichtige Labels |
| `text-3` | `oklch(71% 0 0)` | `#A0A0A0` | Standard-Fließtext, Beschreibungen |
| `text-4` | `oklch(63% 0 0)` | `#8A8A8A` | Meta, Zeitstempel, Panel-Meta |
| `text-5` | `oklch(42% 0 0)` | `#4D4D4D` | nur dekorativ, Platzhalter |
| `text-0` | = `bg-4` | `#000000` | Text auf Akzent- oder Statusflächen |

**Kein Reinweiß.** Heller Text auf echtem Schwarz überstrahlt auf OLED (Halation und
Blooming, besonders bei Astigmatismus). Darum ist `text-1` auf 96 % gedeckelt und bleibt
kleinen Titeln vorbehalten; Fließtext liegt bei 71 %, Meta bei 63 %. Große Flächen Text
(Listen, Beschreibungen) nutzen nie `text-1`.

### Rahmen

| Token | Wert | Verwendung |
|:--|:--|:--|
| `border` | `oklch(29% 0 0)` ≈ `#2A2A2A` | Panel-Rahmen (1 px), Buttons, Inputs |
| `border-light` | `oklch(19% 0 0)` ≈ `#141414` | Trennlinien innerhalb von Panels (1 px) |
| `button-border` | = `border` | Buttons, Inputs |
| `border-hover` | = `accent-2` | Hover/Fokus von Panels und Buttons |

Rahmen sind **deckend** statt transparent, damit sie auf Schwarz zuverlässig sichtbar bleiben.
Sie bleiben trotzdem dunkel: Statische Linien brennen auf OLED am ehesten ein.

### Akzent

- **Mint, Hue 182** (`accent-1` … `accent-5`). `accent-2` ist der Signalton: Panel-Labels,
  Hover-Rahmen, Fokus, kleine Marker, primäre Aktion.
- **`accent-new` (Rosa-Rot, Hue 19)** für alles, was sonst rot wäre: ungültige Eingaben,
  Mute, Badges, destruktive Aktionen.
- **Erwähnungen/Antworten** werden mit einem horizontalen Verlauf von links hinterlegt:
  `linear-gradient(to right, color-mix(in oklch, var(--accent-2), transparent 85%) 40%, transparent)`.
  Für Antworten dieselbe Formel mit `text-3`. Der Verlauf endet in Transparenz, also in Schwarz.
- Die Pastelltöne (Chroma ≤ 0.06 für den Akzent) sind für OLED ideal: Sie leuchten auf
  Schwarz klar, ohne zu flimmern wie gesättigte Neonfarben.

### Status

`online` grün (170), `idle` gelb (90), `dnd` = `accent-new`, `streaming` = Akzent,
`offline` = `text-4`.

### Theming-Regel

Für eine andere Akzentfarbe nur den Hue der `accent-*`-Tokens ändern (z. B. 215 für Blau,
310 für Violett). L und C bleiben unverändert, damit Kontraste nicht kippen.

## Typography

- **Eine Familie für alles:** `DM Mono` (Google Fonts). Fallback:
  `"DM Mono", ui-monospace, "JetBrains Mono", "SF Mono", Menlo, monospace`.
- **Grundgewicht 400 statt 300.** Das Original nutzt Light (300). Auf OLED wirken sehr dünne
  helle Striche auf Schwarz ausgefranst und flimmern bei PWM-Dimmung. Wer den Original-Look
  will, nimmt 300 nur ab 16 px und nur in `text-3` oder dunkler.
- Fett (500) für Überschriften, Namen und Panel-Labels. 700 kaum verwenden.
- **Leicht negatives Tracking:** Das Original nutzt `letter-spacing: -0.05ch`
  (≈ `-0.03em` bei DM Mono). Code-Blöcke bekommen Tracking 0.
- **Zeilenhöhe 1.4** für Fließtext: Etwas mehr Luft gleicht den Überstrahl-Effekt
  heller Glyphen auf Schwarz aus.
- **Kleinschreibung bevorzugen** für Labels und UI-Bezeichner (`channels`, `members`,
  `settings`), wie Befehle im Terminal.
- **Hierarchie über Helligkeit, nicht über Größe.** Größenstufen bleiben eng
  (12 / 14 / 16 / 20 px); die Abstufung `text-1` → `text-5` trägt die Hierarchie.
- **ASCII-Titel:** Große Titel (z. B. der Kopf eines leeren Kanals oder eine Startseite) dürfen
  als ASCII-Art/Figlet-Schrift gesetzt werden, in `text-2` oder `accent-1`.

## Layout

- **Panel-Raster:** Die App ist ein Raster aus nebeneinander und übereinander liegenden Panels
  (z. B. Serverliste | Kanalliste | Inhalt | Mitgliederliste). Jedes Panel ist ein eigener
  gerahmter Kasten.
- **Einheitlicher Gap von 12 px** (`--gap`) zwischen allen Panels **und** zum Fensterrand.
  Gaps sind schwarz wie die Panels; der Abstand zwischen zwei Rahmenlinien macht die
  Trennung sichtbar.
- **Minimaler Chrome:** Die Titelleiste ist nur so hoch wie der Gap (12 px). Header-Buttons
  wandern in die Titelzeile des Inhalts-Panels, statt eine eigene Leiste zu bilden.
- **Kompaktes User-Panel:** Statuszeile (Avatar, Name, Mikro/Audio/Settings) als schmale
  Zeile am unteren Ende der Seitenleiste.
- **Innenabstände** auf dem 4-px-Raster: 4 / 8 / 12 / 16 / 24 px. Listeneinträge dicht
  (4 px vertikal, 8 px horizontal).
- **Ausrichtung am Zeichenraster:** Wo möglich Einrückungen und Abstände in `ch` denken,
  damit Text spaltenweise fluchtet.

## Elevation & Depth

Es gibt **keine Schatten**. Schatten sind auf Schwarz ohnehin unsichtbar. Tiefe entsteht nur
durch Linien:

1. **Rahmen:** Panels haben einen `2px`-Rahmen in `border`. Beim Hover über ein Panel wechselt
   der Rahmen auf `border-hover` (= `accent-2`) mit `transition: border-color 0.2s ease`.
   Das ist das zentrale Fokus-Signal des Systems.
2. **Floating-Elemente** (Menüs, Tooltips, Popouts, Modals) sind ebenfalls **schwarz** und
   heben sich nur über ihren Rahmen ab: `2px solid var(--border)`. Popouts, die ein
   Panel überlagern, bekommen einen hellen Rahmen in `text-5` (`oklch(42% 0 0)`), damit die
   Kante eindeutig ist.
3. **Modals:** Overlay `oklch(0% 0 0 / 0.8)`; das Modal selbst ist ein gerahmtes schwarzes Panel.
4. **Kein Blur/Glas.** `backdrop-filter` kostet auf OLED Energie, hellt Schwarz auf und
   widerspricht dem Prinzip „Pixel aus“.

## Shapes

- **Radius 0 – überall.** Panels, Buttons, Inputs, Menüs, Tooltips, Bilder, Avatare.
- **Avatare quadratisch**, ohne Dekorationsrahmen.
- **Linienstärken:** `1px` für Panel-Rahmen, Buttons, Inputs und Trennlinien innerhalb von
  Panels, `2px` für Fokus-Outlines, `4px` für Divider und Akzentleisten.
- **Statusindikatoren** als kleine Quadrate (8 px) statt Kreise.
- Icons: schlichte Linien-Icons in `text-4`; wo es passt, durch Textzeichen ersetzen
  (`>`, `#`, `@`, `+`, `×`, `•`).

## Components

### Panel
Rechteckiger Container: Fläche `bg-4` (Schwarz), `1px solid var(--border)`, Radius 0,
Innenabstand 8 px. Hover/Fokus → Rahmen `accent-2` (0.2 s ease). Panels scrollen intern;
Scrollbars schmal (4–6 px), eckig, Thumb in `active`, Track transparent.
Scrollbars **nur beim Scrollen oder Hover einblenden**, weil dauerhaft sichtbare Thumbs
einbrennen können.

### Panel-Label
Das Markenzeichen von system24. Ein kurzes Label in Kleinschreibung sitzt **im oberen Rahmen**
des Panels, linksbündig, und unterbricht die Linie, wie ein Fieldset-`legend` oder ein
TUI-Fenstertitel.

- Typografie `label` (12 px, 500), Farbe `accent-2`.
- Position: `position: absolute; top: 0; left: 1ch; transform: translateY(-50%)`.
- Hintergrund Schwarz (`bg-4`) mit `padding: 0 0.5ch`, damit der Rahmen
  hinter dem Text verschwindet.
- Rechts auf derselben Linie dürfen ein Meta-Text in `text-4` und kleine gerahmte Buttons
  sitzen (z. B. `minimize`), nach demselben Prinzip in den Rahmen gesetzt.

### Listeneintrag (Kanäle, Dateien, Menüpunkte)
Text `text-4`, Hover → Hintergrund `hover` + Text `text-3`, ausgewählt → `active` + `text-1`.
Alternativ zur Fläche: eine `2px`-Leiste in `accent-2` am linken Rand. Das ist besonders
OLED-freundlich, weil keine Fläche aufleuchtet. Stumme/inaktive Einträge in `text-5`.
Übergang `0.2s ease`. Präfixe wie `#` oder `>` statt Icons sind erwünscht.

### Buttons
- **Neutral:** `bg-2`, `1px solid var(--button-border)`, Text `text-2`, Hover `bg-1`.
- **Ghost (bevorzugt für Sekundäraktionen):** schwarz, `1px solid var(--button-border)`,
  Hover → Rahmen `accent-2`.
- **Akzent:** `accent-3`, Text `text-0` (Schwarz auf Pastell), Hover `accent-4`,
  gedrückt `accent-4` plus `inset 0 0 0 2px var(--accent-1)` (nicht `accent-5`: Schwarz
  darauf erreicht nur 4.1:1). Nur für die primäre Aktion, weil es die hellste Fläche im UI ist.
- **Gefahr:** `accent-new`, Text `text-0`.
- **Icon-Buttons:** ohne Fläche, Icon `text-4`, Hover → `text-2`.
- Alle Buttons eckig, Beschriftung klein geschrieben.

### Player-Leiste
Jedes Bedienelement hat einen Rahmen und dieselbe Höhe (`32px`); Hover überall gleich:
Rahmen `accent-2`, Text `text-1`. Play ist der Akzent-Button (primäre Aktion), die übrigen
Transport-Icons sind gezeichnet (Strich `1.75` auf 24 px) und quadratisch. Shuffle und Repeat
sind an, wenn Rahmen und Icon den Akzent tragen; Repeat-one zeichnet eine kleine „1“ ins
Icon. Das Geräte-Element („out <gerät>“) hat einen Akzentrahmen, solange dieser Rechner
spielt, sonst `border`. Der Rahmen der Lautstärke leuchtet anteilig (halbe Lautstärke = halber
Rahmen in `accent-2`). Keine Tastenhinweise in Tooltips; Tasten stehen in Einstellungen →
shortcuts.

### Toast
Schwarz, `1px`-Rahmen `accent-2`, `32px` hoch, Text `text-3`, über der Player-Leiste
zentriert. Rein: 260 ms (Deckkraft 0→1, `translateY(14px)`→0, `scale(.96)`→1,
`cubic-bezier(.2,.9,.25,1.15)`); raus: 200 ms ease-in (→ `translateY(8px)`, `scale(.98)`);
sichtbar 2,2 s (mit „undo“ 6 s). Eine `1px`-Linie am unteren Rand (Akzent zu 50 % auf Schwarz
gemischt, deckend) läuft über die Zeit von rechts nach links ab. Eine neue Meldung ersetzt den
Text, pulsiert (`scale` 1→1.04→1, 220 ms) und startet die Zeit neu. Ohne Animationen oder mit
reduzierter Bewegung: keine Bewegung, keine Linie.

### Eingabefeld
Schwarz, Radius 0, Rahmen `1px solid var(--button-border)`, Fokus → Rahmen `accent-2`.
Placeholder `text-5`. Caret gerne als Block (`caret-shape: block`, wo unterstützt), in `accent-2`.

### Nachricht / Feed-Eintrag
Name `text-2` (500), Inhalt `text-3` (400), Zeitstempel `text-5` (`meta`).
Hover → `hover`. Erwähnung → Akzentverlauf von links plus `4px`-Leiste in `accent-2`
am linken Rand. Neue-Nachrichten-Trenner: `4px`-Linie in `accent-new` mit kleinem Label.

### Popout / Menü / Tooltip
Schwarz, `2px`-Rahmen in `border` (über Panels: `text-5`), Radius 0, keine Schatten.
Menüpunkte wie Listeneinträge.

### Medien & Bilder
Große helle Bilder sind die größte Helligkeitsquelle. Vorschaubilder bekommen einen
`1px`-Rahmen in `border-light` und optional `filter: brightness(0.9)`. Bei Hover wird
auf volle Helligkeit geblendet.

### Fortschrittsbalken
Textbasiert statt grafisch, z. B. `[██████████░░░░░░] 1:24 / 3:30`. Gefüllter Teil in
`accent-2`, Rest in `text-5`.

### Ladeanimation
ASCII-Loader: ein rotierendes Zeichen (`| / - \`), eine Braille-Spinner-Sequenz
(`⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`) oder ein ASCII-Logo, das zeilenweise aufgebaut wird.

### Badges & Zähler
Eckige Kästchen, `accent-new` mit `text-0`, Typografie `label`.

## Do's and Don'ts

**Do**
- `#000000` für alle großen Flächen: Panels, Gaps, Popouts, Inputs.
- Jedes Panel rahmen und mit einem kleingeschriebenen Label im oberen Rahmen versehen.
- Den 12-px-Gap konsequent einhalten, zwischen Panels und zum Fensterrand.
- Hierarchie über die Graustufen `text-1` … `text-5` aufbauen.
- Den Akzent sparsam einsetzen: Hover-Rahmen, Fokus, Links, Erwähnungen, primäre Aktion.
- Auswahlzustände eher als Linie/Leiste statt als Fläche zeigen.
- Farben in OKLCH definieren und Akzentvarianten nur über den Hue ableiten.
- Textzeichen (ASCII, Box-Drawing `─│┌┐└┘`, Braille) als gestalterisches Mittel nutzen.
- Animationen kurz und funktional halten (0.2 s ease), mit Schalter für reduzierte Bewegung
  (`prefers-reduced-motion`).

**Don't**
- Keine fast-schwarzen Grautöne zwischen L 1 % und 17 % (Smearing, Banding).
- Kein Reinweiß (`#FFFFFF`), weder für Text noch für Flächen.
- Keine großen hellen Flächen: keine hellen Header, Banner oder vollflächigen Akzentblöcke.
- Keine dauerhaft sichtbaren hellen statischen Flächen (Logos, Badges, Rahmen in `text-2`
  oder heller). Statische Rahmen bleiben dunkel und neutral; `text-1` nur für kurze Titel.
- Kein Blur, kein Glas, keine transparenten Panels.
- Keine abgerundeten Ecken, keine runden Avatare, keine Pillen-Buttons.
- Keine Schatten, kein Glow, keine Neumorphismus-Effekte.
- Keine proportionalen Schriften, auch nicht für Überschriften.
- Keine gesättigten Neonfarben; der Akzent bleibt pastellig (Chroma ≤ 0.145).
- `text-5` nicht für wichtige Informationen verwenden, da der Kontrast nur ≈ 2.5:1 beträgt.

## OLED-Regeln

1. **Pixel aus ist der Normalzustand.** Schwarz ist der Default für jede Fläche. Helligkeit
   ist ein Signal und kein Hintergrund.
2. **Burn-in-Schutz:** Alles, was dauerhaft an derselben Stelle steht (Rahmen, Icons,
   Statuszeile), bleibt neutral grau; Panel-Labels im Akzent sind klein und dünn. Große
   helle Flächen erscheinen nur temporär (Hover, Fokus, neue Inhalte).
3. **Anti-Smearing:** Nicht-schwarze Flächen haben L ≥ 18 %. Keine Verläufe, die über
   größere Flächen von Schwarz in sehr dunkles Grau laufen.
4. **Anti-Blooming:** Text maximal L 96 % und nie Reinweiß, Schriftgewicht ≥ 400 für
   Fließtext. Lange Texte bleiben bei L ≤ 92 %.
5. **Energie:** Die hellste Fläche im UI ist der primäre Akzent-Button. Es gibt keine
   zweite gleich helle Fläche.
6. **Optional „Ultra“-Modus:** `hover`/`active` als reine Linien (linke `2px`-Leiste
   bzw. Rahmen) statt Flächen, `bg-2` → Schwarz mit Rahmen. Damit leuchten nur Text und Linien.

## Barrierefreiheit

Kontrast auf Schwarz (WCAG): `text-1` 18.4:1 · `text-2` 16.8:1 · `text-3` 8.0:1 ·
`text-4` 6.1:1 · `accent-2` 12.7:1 · `accent-new` 10.0:1 · `text-5` 2.5:1 (nur dekorativ).
Auf `active` (`#1F1F1F`) bleibt `text-1` bei ≈ 15:1. Akzent-Buttons mit Schwarz auf
`accent-2`: 12.7:1. Fokus muss immer sichtbar sein (`outline: 2px solid var(--accent-2)`),
nicht nur über Hover.

## CSS-Referenz

```css
@import url('https://fonts.googleapis.com/css2?family=DM+Mono:wght@300;400;500&display=swap');

:root {
  color-scheme: dark;
  --font: 'DM Mono', ui-monospace, monospace;
  --gap: 12px;
  --border-thickness: 1px;
  --divider-thickness: 4px;
  --transition: 0.2s ease;

  --text-0: oklch(0% 0 0);
  --text-1: oklch(96% 0 0);
  --text-2: oklch(92% 0 0);
  --text-3: oklch(71% 0 0);
  --text-4: oklch(63% 0 0);
  --text-5: oklch(42% 0 0);

  --bg-1: oklch(24% 0 0);
  --bg-2: oklch(19% 0 0);
  --bg-3: oklch(0% 0 0);
  --bg-4: oklch(0% 0 0);

  --hover: oklch(19% 0 0);
  --active: oklch(24% 0 0);
  --active-2: oklch(29% 0 0);

  --accent-hue: 182;
  --accent-1: oklch(88% 0.06 var(--accent-hue));
  --accent-2: oklch(83% 0.06 var(--accent-hue));
  --accent-3: oklch(78% 0.06 var(--accent-hue));
  --accent-4: oklch(73% 0.06 var(--accent-hue));
  --accent-5: oklch(68% 0.06 var(--accent-hue));
  --accent-new: oklch(78% 0.09 19);

  --border-light: oklch(19% 0 0);
  --border: oklch(29% 0 0);
  --border-hover: var(--accent-2);
  --button-border: var(--border);
}

body {
  margin: 0;
  padding: var(--gap);
  background: var(--bg-3);
  color: var(--text-3);
  font-family: var(--font);
  font-weight: 400;
  line-height: 1.4;
  letter-spacing: -0.05ch;
  -webkit-font-smoothing: antialiased;
}

.panel {
  position: relative;
  background: var(--bg-4);
  border: var(--border-thickness) solid var(--border);
  border-radius: 0;
  padding: 8px;
  transition: border-color var(--transition);
}
.panel:hover,
.panel:focus-within { border-color: var(--border-hover); }

.panel[data-label]::before {
  content: attr(data-label);
  position: absolute;
  top: 0;
  left: 1ch;
  transform: translateY(-50%);
  padding: 0 0.5ch;
  background: var(--bg-4);
  color: var(--accent-2);
  font-size: 0.75rem;
  font-weight: 500;
  line-height: 1;
  transition: color var(--transition);
}

.list-item[aria-selected="true"] {
  background: var(--active);
  color: var(--text-1);
  box-shadow: inset 2px 0 0 var(--accent-2);
}

/* Scrollbars nur bei Bedarf, gegen Burn-in */
.panel { scrollbar-width: thin; scrollbar-color: transparent transparent; }
.panel:hover { scrollbar-color: var(--active) transparent; }

:focus-visible { outline: 2px solid var(--accent-2); outline-offset: 0; }

@media (prefers-reduced-motion: reduce) {
  * { transition: none !important; animation: none !important; }
}
```

---

*Vorlage: [refact0r/system24](https://github.com/refact0r/system24) (v2.1.0), „a tui-style discord theme“.
Akzent- und Basispalette unverändert aus `system24.theme.css`; Graustufen, Rahmen und Zustände
für OLED neu abgestimmt. Hex-Angaben sind aus OKLCH umgerechnete Näherungen.*
