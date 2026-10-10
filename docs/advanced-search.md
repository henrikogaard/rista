# Advanced search

Project search accepts independent terms joined with implicit `AND`; uppercase
`OR` has lower precedence. Parentheses group terms, and prefix `-` excludes a
term or group. Double quotes search a phrase (`"planning meeting"`); backslash
escapes a quote. A safe regular expression is written between slashes, for
example `/plan.*/`; `\/` matches a slash. Expressions are limited to 4 KiB,
512 tokens, and 32 nested groups. Regular expressions use Rust's bounded-time
engine; look-around and backreferences are not supported.

Supported filters include `tag:`, `path:`, `file:`, `task:`, `task-todo`,
`task-done`, `content:`, `line:`, `block:`, and `section:`. Add `:text` to
either task-state filter to match task text, for example `task-todo:review`.
Bracket
expressions match frontmatter properties: `[status]`, `[status:ready]`,
`[score>=3]`, or `[status:(ready OR waiting)]`. A null property is distinct
from an empty string (`[status:""]`), an empty list (`[status:[]]`), or a
missing property. `path:` and `file:` also match indexed non-note files;
content, task, and tag searches read supported Markdown and `.base` files.
`line:`, `block:`, `section:`, and `task:` groups require all terms to match
within one corresponding region.

Saved-search bookmarks are deferred to follow-up issue #281.
