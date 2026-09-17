function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/** Small, dependency-free markdown subset for overlay answers. */
export function renderLiteMarkdown(source: string): string {
  const escaped = escapeHtml(source);
  const parts = escaped.split(/```([\s\S]*?)```/g);
  return parts
    .map((part, index) => {
      if (index % 2 === 1) {
        const newline = part.indexOf("\n");
        const code = newline === -1 ? part : part.slice(newline + 1);
        return `<pre><code>${code.trimEnd()}</code></pre>`;
      }
      return part
        .replace(/`([^`]+)`/g, "<code>$1</code>")
        .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
        .replace(
          /\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/g,
          '<a href="$2" target="_blank" rel="noreferrer">$1</a>',
        )
        .replace(/^### (.+)$/gm, "<h3>$1</h3>")
        .replace(/^## (.+)$/gm, "<h2>$1</h2>")
        .replace(/^[\-*] (.+)$/gm, "<li>$1</li>")
        .replace(/(<li>[\s\S]+?<\/li>)/g, "<ul>$1</ul>")
        .replace(/\n{2,}/g, "</p><p>")
        .replace(/\n/g, "<br />");
    })
    .join("")
    .replace(/^/, "<p>")
    .replace(/$/, "</p>");
}
