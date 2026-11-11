# Adding known software to a preset

A preset groups programs that the storage and memory views total together.
`ai.ts` is the first one. Each entry in a preset is a **rule**, and one rule
covers one program. Most contributions are a single rule, and no view code
changes.

## When to add a rule

Add a rule when you find a program that:

- keeps a lot of data on disk (its folder shows up big in App storage), or
- uses a lot of memory (it shows up in the Processes view), and
- belongs in the preset's theme. For AI software, that means assistants,
  coding agents, local model runners and their helpers.

## Step 1: Storage (catalog entry)

Storage rules point at entries in the App storage catalog,
`crates/app-audit/catalogs/macos.json`. If the program has no entry yet, add
one. Use the existing entries as examples.

- `id`: a stable kebab-case id, unique in the file.
- `locations`: list the specific folders first, then the data root. Use the
  `classification` and `purgeable` values that already appear in the file.
  Only mark something `purgeable: true` if deleting it is safe and the program
  recreates it. Databases and history are never purgeable.
- `comment`: say what the folder holds and what deleting it costs. Do not
  guess. If you have not verified the contents, write that.

Confirm the paths exist on a real machine first, with `du -sh <path>`.

## Step 2: Preset rule

Append an object to `rules` in the preset file (for AI, `ai.ts`):

```ts
{
  id: 'my-tool',              // unique within the preset
  name: 'My Tool',            // shown in the UI
  storage: ['my-tool'],       // catalog ids from step 1, or [] if none
  match: {
    names: ['my-tool'],       // exact process names, case-insensitive
    commands: [/\/My Tool\.app\//], // patterns against the full command line
  },
}
```

Matching guidance:

- Prefer an app bundle path in `commands` (`/Name.app/`). It is the most
  reliable signal on macOS.
- Use `names` for CLIs. A command pattern that is too broad will pull in
  unrelated processes, so anchor it to a path segment (`/tool/`, not `tool`).
- A rule with a command pattern like `/\/Foo\//` matches anything under a
  `Foo` folder. Check the Processes view for false positives before you commit.

## Step 3: Check

```bash
cd packages/web-ui && node --test 'test/*.test.ts'
```

Open the Processes view, find the program, and confirm its processes appear
in the AI card. Check the App storage view for the catalog entry.

## Presets

- `ai.ts`: AI software (`AI_PRESET`).

To add a new preset (for example "Browsers"), create a file beside `ai.ts`
with a `SoftwarePreset` and add it to `SOFTWARE_PRESETS` in `index.ts`. The
views pick it up automatically.
