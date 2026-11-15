// Adapted from Sahand Sojoodi's X to PDF 1.0.1 (MIT).
// See licenses/XtoPDF-MIT.txt and NOTICE.md.
(() => {
  "use strict";
  const selectors = [
    '[data-testid="twitterArticleReadView"]',
    '[data-testid="twitterArticleRichText"]',
    '[data-testid="articleNoteTweet"]',
    'article [data-testid="longformRichTextComponent"]'
  ];
  const root = selectors.map(s => document.querySelector(s)).find(Boolean);
  if (!root) return {error: "No X Article found. Finish signing in, open the full article and try Capture again. Ordinary posts and threads are unsupported."};
  const clean = s => String(s || "").replace(/\u00a0/g, " ").replace(/\s+/g, " ").trim();
  const inline = (node, depth = 0) => {
    if (depth > 24) throw new Error("Article formatting is too deeply nested.");
    if (node.nodeType === 3) return [{kind: "text", text: node.textContent || ""}];
    if (node.nodeType !== 1) return [];
    if (["SCRIPT", "STYLE", "IFRAME", "OBJECT", "SVG", "BUTTON", "INPUT"].includes(node.tagName)) return [];
    if (node.tagName === "BR") return [{kind: "br"}];
    const children = [...node.childNodes].flatMap(n => inline(n, depth + 1));
    const kind = {B: "strong", STRONG: "strong", I: "em", EM: "em", S: "s", CODE: "code", SUP: "sup", SUB: "sub"}[node.tagName];
    if (kind) return [{kind, children}];
    if (node.tagName === "A") return [{kind: "link", href: node.href || "", children}];
    return children;
  };
  // The article title usually lives outside the rich-text body (and outside
  // any h1), so look in the whole read view first. document.title has the form
  // `Author on X: "Title" / X`; never fall back to just the author's name.
  const scope = root.closest('[data-testid="twitterArticleReadView"]') || root.closest("article") || root;
  const titleNode = scope.querySelector('[data-testid="twitter-article-title"]')
    || document.querySelector('[data-testid="twitter-article-title"]')
    || root.querySelector("h1") || scope.querySelector("h1");
  const documentTitle = () => {
    const raw = clean(document.title).replace(/^\(\d+\+?\)\s*/, "").replace(/\s*\/\s*(X|Twitter)$/i, "");
    const quoted = raw.match(/^.+? on (?:X|Twitter): [“"](.+)[”"]$/i);
    if (quoted) return quoted[1];
    return / on (X|Twitter)$/i.test(raw) ? "" : raw;
  };
  // Article media is served from pbs.twimg.com. Avatars, banners and emoji are
  // page chrome, not article content.
  const imageSource = img => {
    let u;
    try { u = new URL(img.currentSrc || img.src || ""); } catch (_) { return null; }
    if (u.protocol !== "https:" || u.hostname !== "pbs.twimg.com") return null;
    if (/^\/(profile_images|profile_banners|hashflags|emoji|card_img)\//.test(u.pathname)) return null;
    if (img.closest('[data-testid^="UserAvatar"], [data-testid="Tweet-User-Avatar"], [data-testid="User-Name"]')) return null;
    return u;
  };
  try {
    const title = clean(titleNode?.textContent) || documentTitle();
    const user = root.closest("article")?.querySelector('[data-testid="User-Name"]') || document.querySelector('[data-testid="User-Name"]');
    const byline = String(user?.innerText || "").split("\n").map(clean).filter(Boolean).slice(0, 2).join(" ");
    // One DOM-ordered traversal. Code blocks and images are emitted in place, never appended.
    const candidates = [...scope.querySelectorAll('[data-block="true"], [data-testid="markdown-code-block"], img')]
      .filter(node => root.contains(node) || node.tagName === "IMG");
    const content = [];
    const seenImages = new Set();
    for (const node of candidates) {
      if (titleNode && titleNode.contains(node)) continue;
      const codeRoot = node.closest('[data-testid="markdown-code-block"]');
      if (codeRoot && codeRoot !== node) continue;
      if (node.tagName === "IMG") {
        const u = imageSource(node);
        // X often renders the same media twice (background div + img, srcset sizes).
        if (!u || seenImages.has(u.pathname) || seenImages.size >= 100) continue;
        seenImages.add(u.pathname);
        const alt = clean(node.alt);
        content.push({kind: "image", src: u.href, alt: alt === "Image" ? "" : alt.slice(0, 500)});
        continue;
      }
      if (codeRoot === node) {
        const text = (node.querySelector("pre")?.textContent || "").trimEnd();
        if (text) content.push({kind: "code", text});
        continue;
      }
      // Render leaf blocks only; nested wrappers otherwise duplicate the text.
      if (node.querySelector('[data-block="true"], [data-testid="markdown-code-block"]')) continue;
      const text = clean(node.textContent);
      if (!text) continue;
      const semantic = node.closest("h1,h2,h3,h4,h5,h6,li,blockquote") || node;
      const tag = semantic.tagName.toLowerCase();
      const kind = /^h[1-6]$/.test(tag) ? "heading" : tag === "li" ? "list-item" : tag === "blockquote" ? "quote" : "paragraph";
      // The title is printed once, in the header.
      if (text === title && (kind === "heading" || !content.length)) continue;
      content.push({kind, level: kind === "heading" ? Number(tag[1]) : 0,
        ordered: kind === "list-item" && semantic.parentElement?.tagName === "OL",
        runs: [...node.childNodes].flatMap(n => inline(n))});
    }
    if (!content.some(b => b.kind !== "image")) return {error: "The article text is not loaded. Scroll through it, then try Capture again."};
    const article = {title: title || "X Article", byline, source: location.href, content};
    if (JSON.stringify(article).length > 2000000) return {error: "This article exceeds the 2 MB capture limit."};
    return {article};
  } catch (error) { return {error: String(error.message).slice(0, 300)}; }
})()
