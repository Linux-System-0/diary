import DOMPurify from 'dompurify';
import MarkdownIt from 'markdown-it';
import taskLists from 'markdown-it-task-lists';

const md = new MarkdownIt({
  html: false,
  linkify: true,
  breaks: true
}).use(taskLists, { enabled: false, label: true, labelAfter: true });

const defaultLinkOpen = md.renderer.rules.link_open;
md.renderer.rules.link_open = (tokens, idx, options, env, self) => {
  const token = tokens[idx];
  token.attrSet('target', '_blank');
  token.attrSet('rel', 'noopener noreferrer nofollow');
  return defaultLinkOpen
    ? defaultLinkOpen(tokens, idx, options, env, self)
    : self.renderToken(tokens, idx, options);
};

md.renderer.rules.image = (tokens, idx) => {
  const token = tokens[idx];
  const src = String(token.attrGet('src') ?? '');
  const alt = token.content || '图片';
  const title = String(token.attrGet('title') ?? '');
  if (!/^https?:\/\//i.test(src)) {
    return escapeHtml(alt);
  }
  const label = title ? `点击加载图片 · ${title}` : '点击加载图片';
  return `<button type="button" class="md-image-placeholder" data-src="${escapeHtml(
    src
  )}" data-alt="${escapeHtml(alt)}" title="${escapeHtml(src)}">🖼 ${escapeHtml(label)}</button>`;
};

function escapeHtml(value: string): string {
  return md.utils.escapeHtml(value);
}

export function renderMarkdown(source: string): string {
  const html = md.render(source);
  return DOMPurify.sanitize(html, {
    ALLOWED_TAGS: [
      'p', 'br', 'hr', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
      'strong', 'em', 'del', 's', 'blockquote', 'pre', 'code',
      'ul', 'ol', 'li', 'a', 'button', 'table', 'thead', 'tbody',
      'tr', 'th', 'td'
    ],
    ALLOWED_ATTR: [
      'href', 'title', 'class', 'alt', 'target', 'rel', 'type',
      'data-src', 'data-alt'
    ],
    FORBID_TAGS: ['script', 'style', 'iframe', 'object', 'embed', 'form', 'input'],
    FORBID_ATTR: ['onerror', 'onload', 'onclick', 'style']
  });
}

/** Loads external images only after an explicit click. */
export function bindImagePlaceholders(container: HTMLElement): () => void {
  const handler = (event: MouseEvent) => {
    const target = (event.target as HTMLElement | null)?.closest<HTMLButtonElement>(
      '.md-image-placeholder'
    );
    if (!target) return;
    const src = target.dataset.src;
    if (!src) return;
    const image = document.createElement('img');
    image.src = src;
    image.alt = target.dataset.alt ?? '';
    image.loading = 'lazy';
    image.referrerPolicy = 'no-referrer';
    target.replaceWith(image);
  };
  container.addEventListener('click', handler);
  return () => container.removeEventListener('click', handler);
}
