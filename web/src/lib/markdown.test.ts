// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';
import { bindImagePlaceholders, renderMarkdown } from './markdown';

describe('markdown sanitization', () => {
  it('does not allow raw HTML or script', () => {
    const html = renderMarkdown(
      '# 标题\n\n<script>alert(1)</script>\n\n<img src=x onerror="alert(2)">'
    );
    expect(html).toContain('<h1>标题</h1>');
    const container = document.createElement('div');
    container.innerHTML = html;
    expect(container.querySelector('script, img, iframe, style')).toBeNull();
    expect(container.textContent).toContain('<script>');
  });

  it('removes dangerous link protocols and hardens external links', () => {
    const html = renderMarkdown(
      '[bad](javascript:alert(1)) [good](https://example.com)'
    );
    const container = document.createElement('div');
    container.innerHTML = html;
    const anchors = [...container.querySelectorAll('a')];
    expect(anchors).toHaveLength(1);
    expect(anchors[0].getAttribute('href')).toBe('https://example.com');
    expect(anchors[0].getAttribute('target')).toBe('_blank');
    expect(anchors[0].getAttribute('rel')).toBe('noopener noreferrer nofollow');
    expect(anchors.some((anchor) => (anchor.getAttribute('href') ?? '').startsWith('javascript:'))).toBe(false);
  });

  it('escapes iframe and style, and refuses data: images', () => {
    const html = renderMarkdown(
      '<iframe src="https://evil.example"></iframe>\n\n<style>body{display:none}</style>\n\n![x](data:image/png;base64,AAAA)'
    );
    expect(html).not.toMatch(/<iframe/i);
    expect(html).not.toMatch(/<style/i);
    expect(html).not.toMatch(/data:image/i);
    expect(html).not.toMatch(/<img/i);
  });

  it('turns external images into click-to-load placeholders', () => {
    const html = renderMarkdown('![猫](https://example.com/cat.png)');
    expect(html).toContain('md-image-placeholder');
    expect(html).toContain('data-src="https://example.com/cat.png"');
    expect(html).not.toContain('<img');

    const container = document.createElement('div');
    container.innerHTML = html;
    document.body.append(container);
    const unbind = bindImagePlaceholders(container);
    container.querySelector<HTMLButtonElement>('.md-image-placeholder')!.click();
    const image = container.querySelector('img');
    expect(image?.getAttribute('src')).toBe('https://example.com/cat.png');
    expect((image as HTMLImageElement).referrerPolicy).toBe('no-referrer');
    unbind();
    container.remove();
  });

  it('renders task list text without interactive inputs', () => {
    const html = renderMarkdown('- [x] 完成\n- [ ] 未完成');
    expect(html).toContain('完成');
    expect(html).not.toContain('<input');
  });
});
