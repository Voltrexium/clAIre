function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function dedentCode(value: string): string {
  const lines = value.split("\n");
  const nonempty = lines.filter((line) => line.trim().length > 0);
  if (!nonempty.length) return value.trimEnd();
  let pad = nonempty[0].match(/^[ \t]*/)?.[0] ?? "";
  if (!pad) return value.trimEnd();
  for (const line of nonempty) {
    const ws = line.match(/^[ \t]*/)?.[0] ?? "";
    let n = 0;
    while (n < pad.length && n < ws.length && pad[n] === ws[n]) n += 1;
    pad = pad.slice(0, n);
    if (!pad) return value.trimEnd();
  }
  return lines
    .map((line) => (line.startsWith(pad) ? line.slice(pad.length) : line))
    .join("\n")
    .trimEnd();
}

type Segment = { kind: "text" | "code"; value: string; lang?: string };

function splitFences(source: string): Segment[] {
  const segments: Segment[] = [];
  let cursor = 0;

  while (cursor < source.length) {
    const start = source.indexOf("```", cursor);
    if (start === -1) {
      segments.push({ kind: "text", value: source.slice(cursor) });
      break;
    }
    const nl = source.indexOf("\n", start + 3);
    if (nl === -1) {
      segments.push({ kind: "text", value: source.slice(cursor) });
      break;
    }
    if (start > cursor) {
      segments.push({ kind: "text", value: source.slice(cursor, start) });
    }
    const lang = source.slice(start + 3, nl).trim().split(/\s+/)[0] || "";
    const bodyStart = nl + 1;
    const close = source.indexOf("```", bodyStart);
    if (close === -1) {
      segments.push({ kind: "code", lang, value: source.slice(bodyStart) });
      break;
    }
    segments.push({ kind: "code", lang, value: source.slice(bodyStart, close) });
    cursor = close + 3;
    if (source[cursor] === "\n") cursor += 1;
  }

  return segments;
}

function inline(escaped: string): string {
  const slots: string[] = [];
  const hold = (html: string) => {
    slots.push(html);
    return `\u0000${slots.length - 1}\u0000`;
  };

  let text = escaped.replace(/`([^`]+)`/g, (_, code: string) => hold(`<code>${code}</code>`));

  text = text.replace(
    /\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/g,
    (_, label: string, href: string) =>
      hold(`<a href="${href}" target="_blank" rel="noreferrer">${label}</a>`),
  );

  text = text.replace(
    /(?<!href=")(https?:\/\/[^\s<)]+)/g,
    (url) => hold(`<a href="${url}" target="_blank" rel="noreferrer">${url}</a>`),
  );

  text = text
    .replace(/~~(?!~)(.+?)~~/g, "<del>$1</del>")
    .replace(/(?<![A-Za-z0-9~])~(?!~)(\S(?:[^\n~]*\S)?)~(?![A-Za-z0-9~])/g, "<del>$1</del>")
    .replace(/\*\*\*(.+?)\*\*\*/g, "<strong><em>$1</em></strong>")
    .replace(/___(.+?)___/g, "<strong><em>$1</em></strong>")
    .replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>")
    .replace(/__(.+?)__/g, "<strong>$1</strong>")
    .replace(/(?<!\*)\*(?!\*)([^*]+)\*(?!\*)/g, "<em>$1</em>")
    .replace(/(?<![A-Za-z0-9_])_(?!_)([^_]+)_(?![A-Za-z0-9_])/g, "<em>$1</em>");

  return text.replace(/\u0000(\d+)\u0000/g, (_, i: string) => slots[Number(i)]);
}

const LIST_RE = /^(\s*)(?:([-*+])|(\d+)[.)])(?: \[([ xX])\])? (.+)$/;
const HR_RE = /^\s*(?:[-*_]\s*){3,}$/;
const HEADING_RE = /^(#{1,4}) (.+)$/;
const QUOTE_RE = /^&gt; ?/;
const TABLE_SEP_RE = /^\s*\|?(?:\s*:?-+:?\s*\|)+\s*:?-+:?\s*\|?\s*$/;

function splitCells(line: string): string[] {
  let row = line.trim();
  if (row.startsWith("|")) row = row.slice(1);
  if (row.endsWith("|")) row = row.slice(0, -1);
  return row.split("|").map((cell) => cell.trim());
}

function isTableStart(lines: string[], index: number): boolean {
  const header = lines[index];
  const sep = lines[index + 1];
  if (!header || !sep || !header.includes("|") || !TABLE_SEP_RE.test(sep)) return false;
  return splitCells(header).length === splitCells(sep).length;
}

function renderTable(lines: string[], start: number): { html: string; next: number } {
  const header = splitCells(lines[start]).map((cell) => `<th>${inline(cell)}</th>`).join("");
  let index = start + 2;
  const body: string[] = [];
  while (index < lines.length && lines[index].includes("|") && !TABLE_SEP_RE.test(lines[index])) {
    if (!lines[index].trim()) break;
    const cells = splitCells(lines[index]).map((cell) => `<td>${inline(cell)}</td>`).join("");
    body.push(`<tr>${cells}</tr>`);
    index += 1;
  }
  return {
    html: `<table><thead><tr>${header}</tr></thead><tbody>${body.join("")}</tbody></table>`,
    next: index,
  };
}

function renderList(lines: string[], start: number): { html: string; next: number } {
  type Frame = { indent: number; ordered: boolean; items: string[] };
  const stack: Frame[] = [];
  let index = start;
  let result = "";

  const flush = (frame: Frame) => `<${frame.ordered ? "ol" : "ul"}>${frame.items.join("")}</${frame.ordered ? "ol" : "ul"}>`;

  const attach = (html: string) => {
    if (!stack.length) {
      result += html;
      return;
    }
    const parent = stack[stack.length - 1];
    const last = parent.items.length - 1;
    parent.items[last] = parent.items[last].replace(/<\/li>$/, `${html}</li>`);
  };

  const closeDeeper = (indent: number) => {
    while (stack.length && stack[stack.length - 1].indent > indent) {
      attach(flush(stack.pop()!));
    }
  };

  const closeSameTypeMismatch = (indent: number, ordered: boolean) => {
    const top = stack[stack.length - 1];
    if (top && top.indent === indent && top.ordered !== ordered) {
      attach(flush(stack.pop()!));
    }
  };

  while (index < lines.length) {
    const match = LIST_RE.exec(lines[index]);
    if (!match) break;
    const indent = match[1].replace(/\t/g, "  ").length;
    const ordered = Boolean(match[3]);
    const task = match[4] !== undefined;
    const checked = match[4]?.toLowerCase() === "x";
    const text = match[5];

    closeDeeper(indent);
    closeSameTypeMismatch(indent, ordered);
    const top = stack[stack.length - 1];
    if (!top || top.indent < indent) {
      stack.push({ indent, ordered, items: [] });
    }

    const marker = task
      ? `<input type="checkbox" disabled${checked ? " checked" : ""} /> `
      : "";
    stack[stack.length - 1].items.push(`<li>${marker}${inline(text)}</li>`);
    index += 1;

    while (index < lines.length) {
      const next = lines[index];
      if (!next.trim() || LIST_RE.test(next) || isBlockStart(next, lines[index + 1])) break;
      if (!(next.startsWith("  ") || next.startsWith("\t"))) break;
      const frame = stack[stack.length - 1];
      const last = frame.items.length - 1;
      frame.items[last] = frame.items[last].replace(
        /<\/li>$/,
        `<br />${inline(next.trim())}</li>`,
      );
      index += 1;
    }
  }

  closeDeeper(-1);
  return { html: result, next: index };
}

function renderQuote(lines: string[], start: number): { html: string; next: number } {
  const inner: string[] = [];
  let index = start;
  while (index < lines.length && QUOTE_RE.test(lines[index])) {
    inner.push(lines[index].replace(QUOTE_RE, ""));
    index += 1;
  }
  const body = renderBlocks(inner.join("\n"));
  return { html: `<blockquote>${body}</blockquote>`, next: index };
}

function isBlockStart(line: string, next?: string): boolean {
  if (!line.trim()) return true;
  if (HR_RE.test(line) || HEADING_RE.test(line) || QUOTE_RE.test(line) || LIST_RE.test(line)) {
    return true;
  }
  return Boolean(next && line.includes("|") && TABLE_SEP_RE.test(next));
}

function renderBlocks(escaped: string): string {
  const lines = escaped.replace(/\r\n/g, "\n").split("\n");
  const out: string[] = [];
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];
    if (!line.trim()) {
      i += 1;
      continue;
    }
    if (HR_RE.test(line)) {
      out.push("<hr />");
      i += 1;
      continue;
    }
    const heading = HEADING_RE.exec(line);
    if (heading) {
      const level = heading[1].length;
      out.push(`<h${level}>${inline(heading[2])}</h${level}>`);
      i += 1;
      continue;
    }
    if (QUOTE_RE.test(line)) {
      const quote = renderQuote(lines, i);
      out.push(quote.html);
      i = quote.next;
      continue;
    }
    if (isTableStart(lines, i)) {
      const table = renderTable(lines, i);
      out.push(table.html);
      i = table.next;
      continue;
    }
    if (LIST_RE.test(line)) {
      const list = renderList(lines, i);
      out.push(list.html);
      i = list.next;
      continue;
    }

    const para: string[] = [line];
    i += 1;
    while (i < lines.length && !isBlockStart(lines[i], lines[i + 1])) {
      para.push(lines[i]);
      i += 1;
    }
    out.push(`<p>${para.map(inline).join("<br />")}</p>`);
  }

  return out.join("");
}

/** Dependency-free markdown subset for overlay answers (headings, lists, code, tables, emphasis). */
export function renderLiteMarkdown(source: string): string {
  if (!source) return "";
  return splitFences(source)
    .map((segment) => {
      if (segment.kind === "code") {
        const langName = escapeHtml(segment.lang || "");
        const langClass = segment.lang ? ` class="language-${langName}"` : "";
        const code = escapeHtml(dedentCode(segment.value));
        return `<div class="md-code"><div class="md-code-bar"><span class="md-code-lang">${langName}</span><span class="copy-code" title="Copy" role="button" tabindex="0" aria-label="Copy code">Copy</span></div><pre><code${langClass}>${code}</code></pre></div>`;
      }
      return renderBlocks(escapeHtml(segment.value));
    })
    .join("");
}
