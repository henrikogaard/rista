# Style snippets

Style snippets are small JSON files in `config_dir()/snippets`. On macOS, the
folder is `~/Library/Application Support/no.ogard.rista/snippets`. Create the
folder if it does not exist and keep one snippet per top-level `.json` file.
Files contain data only; CSS, scripts, and network access are not supported.

Save this JSON as `paper.json`:

```json
{
  "colors": {
    "foreground": "#242424",
    "background": "#fafafa"
  },
  "dark_colors": {
    "foreground": "#f3f3f3",
    "background": "#171717"
  },
  "typography": {
    "body_font": "New York",
    "editor_font": "New York",
    "code_font": "SF Mono",
    "heading_font": "New York",
    "body_size": 17,
    "line_height": 1.5,
    "heading_sizes": [30, 26, 22, 19, 17, 15],
    "heading_weights": [700, 650, 600, 550, 500, 450]
  },
  "readable_width": 760,
  "callout_tokens": { "warning": "warning" },
  "tag_token": "link"
}
```

Select it for one note with this frontmatter:

```yaml
---
cssclasses: [paper]
---
```

`colors` uses the known theme color names. `light_colors` and `dark_colors`
override common colors for one mode. Color values are six- or eight-digit
hexadecimal values. Typography accepts body, editor, code, and heading font
families (non-empty, at most 128 UTF-8 bytes), body size (8–72 px), line height
(1–3), six optional heading sizes (8–120 px each), and six optional heading
weights (100–900 each). Readable width is measured in pixels (240–2000). A
`code_font` affects fenced code blocks; the public text API does not expose an
inline-code font-family override. Callout and tag colors refer to finite
semantic names such as `foreground`, `muted.foreground`, `link`,
`info.background`, `warning`, `danger`, `success`, `accent.background`,
`background`, and `border`. Callout keys use supported Markdown kinds:
`note`, `abstract`, `summary`, `tldr`, `info`, `todo`, `tip`, `hint`,
`important`, `success`, `check`, `done`, `question`, `help`, `faq`, `warning`,
`caution`, `attention`, `failure`, `fail`, `missing`, `danger`, `error`, `bug`,
`example`, `quote`, and `cite`.

The Settings switch enables a snippet globally. Matching `cssclasses` in a
note's frontmatter also applies a valid snippet to that note, independently of
the switch. Global snippets are applied in sorted filename order; note-local
classes are then applied in their listed order, once each. Global colors apply
to application chrome through the selected theme. Note-local colors are
limited to note roles: body foreground, links, callouts and tags, preview and
editor backgrounds, fenced-code background, inline-code background, and table
header colors. Other note-local tokens do not affect workspace chrome. Source
editor syntax colors remain unchanged. Local typography and readable width
affect only the note. `wide` disables the width cap; `narrow` and `readable`
force it. A snippet `readable_width` replaces the normal 760 px cap.

For sparse heading size or weight arrays, `null` preserves the value from an
earlier snippet or the built-in style.

Files are reloaded while Rísta runs after changes settle. Invalid JSON, invalid
values, unknown fields or color tokens, unreadable files, symbolic links,
non-regular files, reserved names, oversized files, and entries beyond the
catalog limit are skipped and shown with a localized warning. Each file is
limited to 64 KiB; only the first 128 filenames in sorted order are loaded,
with one aggregate warning for additional entries.
Missing enabled files remain in Settings and become active again if restored.
