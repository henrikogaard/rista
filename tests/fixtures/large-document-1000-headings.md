# Large Document Benchmark Fixture
# Heading 2
Some content paragraph that goes on long enough to be interesting for markdown parsing benchmarks.

## Level 2 Heading 1
With a **bold** and *italic* and `inline code` and a [link](https://example.com).

### Level 3 Heading 1
> Blockquote with *formatting* inside it.

## Level 2 Heading 2
- List item 1
- List item 2
  - Nested list item
  - Another nested

### Level 3 Heading 2
1. Ordered item one
2. Ordered item two

#### Level 4 Heading

##### Level 5 Heading

###### Level 6 Heading

Some paragraph with a long sentence that goes on for a while to test rendering performance of the markdown pipeline when facing longer text content without any special formatting.

---

> [!note] A callout
> This is an Obsidian-compatible callout block with some content.

---

| Column A | Column B | Column C | Column D |
|----------|----------|----------|----------|
| Cell 1   | Cell 2   | Cell 3   | Cell 4   |
| Cell 5   | Cell 6   | Cell 7   | Cell 8   |

---

```
A code block with some code in it
const x = 1
const y = 2
function test() { return x + y }
```
