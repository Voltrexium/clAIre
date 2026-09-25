import { expect, test } from "vitest";
import { renderLiteMarkdown } from "./markdown";
import { usageKeyId } from "./usageKey";

test("usage ids match the Rust SHA-256 fingerprint", () => {
  expect(usageKeyId("abc")).toBe(
    "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
  );
  expect(usageKeyId("  abc  ")).toBe(usageKeyId("abc"));
  expect(usageKeyId("local")).toBe("local");
  expect(usageKeyId("")).toBe("");
  const id = usageKeyId("sk-test");
  expect(id.startsWith("sha256:")).toBe(true);
  expect(id.includes("sk-test")).toBe(false);
});

test("answer markdown escapes html", () => {
  const html = renderLiteMarkdown("Use <script>alert(1)</script>");
  expect(html.includes("<script>")).toBe(false);
  expect(html.includes("&lt;script&gt;")).toBe(true);
});
