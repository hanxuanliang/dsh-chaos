# Frontend foundations

This document is the source of truth for the visual language, component
boundaries, and client-side file organization of dsh-chaos.

The product is a dense collaboration workspace embedded in DeepSeek Harness.
It is not a standalone application. DSH owns the shell and visual skin;
dsh-chaos owns collaboration-specific information architecture and behavior.

## 1. Design direction

The target is calm, compact, and native to DSH:

- Design variance: 3/10. Prefer predictable alignment and clear hierarchy.
- Motion intensity: 2/10. Motion communicates state changes only.
- Visual density: 8/10. Favor rows, dividers, and progressive disclosure over
  decorative cards.
- Theme: inherit DSH light and dark themes without a plugin-specific palette.

The reference priority is:

1. Current DSH host behavior and `@deepseek-ai/dsh-client-ui-primitives`.
2. Established DSH plugin patterns.
3. Established dense-list and responsive split-view interaction patterns.
4. Product-specific behavior required by the collaboration domain.

When references disagree, the higher item wins.

## 2. Adopted interaction patterns

We adopt these patterns:

| Pattern | Chaos use |
| --- | --- |
| Compact header and list rows | Activity, Channel, Agent, and Task indexes |
| List/detail split view | Activity detail workflows |
| Resizable desktop panels | Activity list/detail and optional Thread context |
| List-to-detail drill-in on narrow screens | Activity and Agents below 700px |
| Compact centered detail dialog | Task information and live Task actions |
| Centered settings sections with grouped rows | Agent Identity and Runtime settings |
| Icon-only secondary actions with menus | Dense toolbars and row actions |

We do not adopt these parts from external application references:

- A separate sidebar, page frame, top navigation, font, or theme palette.
- A custom chat composer. Chaos uses the official DSH composer geometry.
- Tailwind, shadcn, third-party icon sets, or another application's local UI components.
- Mock-only actions or state that has no real Chaos RPC behind it.
- Decorative labels, status colors, or cards that do not encode product state.

This is a structural reference, not a skin transplant.

## 3. Visual language

### 3.1 Color

Use DSH semantic tokens directly. Do not copy an external palette and do not
introduce raw color values.

- Surfaces: `--dsw-alias-bg-base`, `--dsw-alias-bg-layer-1`,
  `--dsw-alias-bg-layer-2`, `--dsw-alias-bg-layer-3`.
- Text: `--dsw-alias-label-primary`, `--dsw-alias-label-secondary`,
  `--dsw-alias-label-tertiary`.
- Borders: `--dsw-alias-border-l1`, `--dsw-alias-border-l2`,
  `--dsw-alias-border-l3`.
- Interaction: `--dsw-alias-interactive-bg-hover`,
  `--dsw-alias-interactive-bg-active`.
- Semantic state: DSH business, success, warning, and error tokens.

Add a `--chaos-*` color only when the collaboration domain has a stable semantic
state that DSH does not represent. Document its meaning next to the declaration.

### 3.2 Typography

Inherit the host font. Hierarchy comes from weight and muted color before size.

| Role | Size / line height | Weight |
| --- | --- | --- |
| Page title | 16 / 24 | 600 |
| Section or panel title | 14 / 20 | 600 |
| Row title and body | 13 / 20 | 400 or 500 |
| Secondary text | 12 / 18 | 400 |
| Metadata, count, timestamp | 11 / 17 | 400 or 500 |
| Code and identifiers | 11 / 16 | host monospace, tabular numbers |

Chat message bodies may remain 16 / 28 where they mirror the official DSH
conversation. Do not apply dense list typography to messages or the composer.

### 3.3 Spacing and geometry

Use a 4px spacing grid: 4, 8, 12, 16, 24, and 32px.

| Element | Contract |
| --- | --- |
| Workspace header | 48px high |
| Compact toolbar | 40px high |
| Compact icon button | 28px square |
| Standard control | 32 or 34px high |
| Navigation row | 34px minimum |
| Dense entity row | 44px minimum |
| Rich Activity row | content-driven, normally 56-68px |
| Chip or segmented control | 5-6px radius |
| Input | 8px radius |
| Structural surface | 10px radius |
| Modal | 12px radius |

Use fully rounded pills only for compact semantic status or counts. Structural
tabs, buttons, fields, and panels use the radius scale above.

### 3.4 Borders, elevation, and motion

- Structural regions use one-pixel DSH borders and no shadow.
- Shadows are reserved for menus, popovers, dialogs, and dragged objects.
- Hover and focus transitions use 120-160ms.
- Overlay entry and panel transitions may use up to 180ms.
- Animate only opacity and transform. Respect `prefers-reduced-motion`.
- Every interactive element supports default, hover, active, focus-visible,
  disabled, and busy states where applicable.

## 4. Layout contracts

### 4.1 Host boundary

DSH owns the global sidebar, conversation chrome, settings shell, theme, and
native composer. Chaos must not add a second application shell inside them.

The Chaos workspace may own:

- one 48px workspace header;
- an internal navigation rail;
- one primary content surface;
- one optional context/detail pane.

### 4.2 Split panes

Desktop split panes must be resizable. Use a headless, keyboard-accessible
resizing primitive rather than custom pointer math. `react-resizable-panels`
3.x is compatible with React 18 and 19 and is the approved candidate, subject
to the normal dependency and bundle review before implementation.

Contracts:

- Activity list: default 320px, minimum 280px, maximum 480px.
- Detail pane: minimum 420px.
- Thread context: default 360px, minimum 300px, maximum 520px.
- Persist sizes by surface, not globally.
- The handle has an 8px hit target, a one-pixel visible rule, keyboard support,
  focus-visible state, and double-click reset.
- Opening and closing a detail pane must not reset the user's saved width.

### 4.3 Responsive behavior

- At 980px and above, show the full rail, primary surface, and optional detail
  pane when space permits.
- From 701px to 979px, collapse optional navigation before compressing content.
- At 700px and below, use list-to-detail drill-in. Never squeeze two working
  panes side by side.
- Narrow detail views include a visible Back action and restore focus to the
  originating row.

## 5. Required component system

### 5.1 Host-backed primitives

Use DSH primitives directly unless Chaos needs one shared behavioral default.

- `Button`
- `Input`
- `Modal`
- `RiskConfirmation`
- `Menu`
- `Tooltip`
- `StateDot`
- official `Icon*Outline*` components

Do not wrap these only to rename them. Add an adapter only when it centralizes a
real contract such as icon-button labelling, busy state, or sizing.

Button roles are fixed:

| Role | Use |
| --- | --- |
| Primary | The single commit or submit action in a surface |
| Outline | Secondary actions that must remain visible |
| Ghost | Toolbar and contextual actions |
| Icon | Compact toolbar action, always 28px with an accessible name |
| Danger | Destructive confirmation only, never routine navigation |

Buttons use only compact 28px and standard 32-34px heights. Labels never wrap.
Busy state preserves the button width, prevents duplicate submission, and does
not disable unrelated actions. Icon-only actions use a tooltip when the glyph
is not universally understood.

### 5.2 Chaos UI primitives

These are the minimal shared components Chaos should own:

| Component | Responsibility |
| --- | --- |
| `IconButton` | Consistent 28px geometry, accessible label, tooltip policy |
| `Field` | Label, control, hint, error, required state, stable ids |
| `SearchField` | Search icon, clear affordance, empty query behavior |
| `Tabs` | Real tab/panel semantics, arrow-key navigation, active indicator |
| `SegmentedControl` | Filters or view modes, button-group semantics and pressed state |
| `StatusChip` | Real domain status only, never decorative tagging |
| `AvatarChip` | One custom local image or a stable, differentiable two-glyph fallback |
| `EntityRow` | Selection, primary/secondary/meta slots, row action slot |
| `PanelHeader` | Title, optional back action, compact action group |
| `Toolbar` | Filters, view controls, and overflow behavior |
| `SplitPane` | Resizable desktop panes plus persisted sizing |
| `ResponsiveDrilldown` | List/detail transition below 700px |
| `EmptyState` | One explanation and at most one non-duplicated action |
| `SkeletonList` | Loading rows shaped like the final content |
| `ErrorBanner` | Inline failure with optional retry action |

Avoid a generic `Card` primitive. Use a surface only when it communicates
containment or elevation. Lists should normally be rows separated by spacing or
a single group divider.

### 5.3 Product components

Product behavior remains inside feature folders:

- Channels: `ChannelRail`, `ChannelView`, `ChannelComposer`, members.
- Activity: filters, `ActivityList`, `ActivityRow`, `ActivityDetail`.
- Messages: `MessageStream`, `MessageRow`, `MessageBody`, reply preview.
- Threads: `ThreadPanel`, root card, thread composer integration.
- Tasks: filters, horizontally scrollable board, lane, card, detail dialog.
- Agents: list, detail, create flow, Identity, Runtime, Collaboration.
- Approvals: request card and resolution state when that vertical is built.

Product components compose shared primitives. Shared primitives never import a
feature, store, RPC client, or domain model.

## 6. Target client organization

### 6.1 Organization status

The client is organized by ownership rather than visual size:

- `entry/` owns host registration, mounting, navigation, and workspace
  composition.
- `data/` owns the typed RPC client, change feed, and client-side store.
- `features/` owns Activity, Agents, Channels, Messages, Threads, and Tasks.
- `shared/ui` and `shared/layout` contain domain-free visual and layout
  contracts.

The former root-level product components and the `atoms/` / `blocks/`
directories have been removed. Larger feature components may still be split
when doing so creates a meaningful behavior boundary; file length alone is not
a reason to introduce another layer.

### 6.2 Target tree

```text
src/client/
  entry/
    index.tsx
    mount.tsx
    navigation.ts
  data/
    api.ts
    events.ts
    store.ts
  shared/
    ui/
      IconButton.tsx
      Field.tsx
      SearchField.tsx
      Tabs.tsx
      SegmentedControl.tsx
      StatusChip.tsx
      EmptyState.tsx
      ErrorBanner.tsx
      SkeletonList.tsx
    layout/
      PanelHeader.tsx
      Toolbar.tsx
      SplitPane.tsx
      ResponsiveDrilldown.tsx
    hooks/
    styles/
      foundations.module.css
  shims/
    node-min.ts
  features/
    activity/
    agents/
    channels/
    messages/
    threads/
    tasks/
```

Rules:

- A feature owns its components, hooks, local styles, and tests.
- Cross-feature protocol types stay outside the client under the existing
  shared contract boundary.
- `shared/ui` contains domain-free pieces only.
- `shared/layout` owns sizing, scrolling, resizing, and responsive transitions.
- CSS Modules stay next to their component.
- No new feature files are added directly under `src/client/`.
- Do not reintroduce `atoms/` or `blocks/`. Those names describe visual size,
  not ownership, and scatter one feature across several directories.
- `shims/` is a build boundary, not a UI layer. `node-min.ts` provides the
  minimal browser-safe `node:path`, `node:process`, and `node:url` exports that
  `react-markdown`'s `vfile` dependency imports. DSH's browser module table does
  not provide Node builtins, so this shim remains required for plugin loading.

## 7. Migration history

The frontend foundation was adopted incrementally in the following order:

1. **Foundation:** added shared sizing/state styles, `IconButton`, `Field`,
   `Tabs`, `SegmentedControl`, `PanelHeader`, and `SplitPane`; removed the broad
   button reset and added focused behavior tests.
2. **Activity:** moved Activity files into one feature and adopted the resizable
   list/detail layout while preserving the current RPC/state contract.
3. **Agents:** moved Agent files into one feature, used the settings section/row
   pattern, and exposed the effective `Full access` Session state.
4. **Tasks:** split the board into filter, board/list,
   lane, item, and detail components.
5. **Channels, Messages, Threads:** moved by vertical and deleted the old
   `atoms/` and `blocks/` directories.
6. **Cleanup:** reduced `CollabPanel` to composition and host integration and removed
   duplicated button, field, empty, loading, and focus rules.

Future structural changes follow the same rule: preserve public RPCs and
behavior, land as reviewable commits, and pass the official-host visual matrix.

## 8. Acceptance matrix

Every migrated surface is checked in the real DSH host:

- 1200px, 900px, and 650px widths.
- Light and dark themes.
- Pointer and keyboard navigation.
- Default, hover, active, focus-visible, disabled, busy, empty, loading, and
  error states that apply to the surface.
- No console errors, page exceptions, failed requests, or accidental host DOM
  styling.
- Typecheck, client build, client smoke, and the relevant browser scenario.

Screenshots prove visual behavior. Tests prove contracts. Neither substitutes
for the other.
