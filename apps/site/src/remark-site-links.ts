/**
 * Rewrites links between the repository's markdown documents into site routes
 * and unlinks every other repository-relative target, so the public site never
 * ships a dead link. External links, site-absolute links, and in-page anchors
 * are kept as-is.
 */

const ROUTES: Record<string, string> = {
  genesis: '/',
  architecture: '/architecture',
  research: '/research',
};

interface SiteNode {
  type: string;
  url?: string;
  children?: SiteNode[];
}

export function remarkSiteLinks() {
  // The transformer doubles as the recursive walker: every remark tree is a
  // SiteNode, so the plugin itself is the entry point of the rewrite.
  function rewriteLinks(node: SiteNode): void {
    // Links do not nest in markdown; do not descend into them.
    if (node.type === 'link') return;
    const children = node.children;
    if (!children) return;
    for (let i = children.length - 1; i >= 0; i--) {
      const child = children[i];
      if (child.type !== 'link') {
        rewriteLinks(child);
        continue;
      }
      const route = routeFor(child.url ?? '');
      if (route !== undefined) {
        child.url = route;
        continue;
      }
      // Unknown repository-relative target: replace the link with its content.
      children.splice(i, 1, ...(child.children ?? []));
    }
  }
  return rewriteLinks;
}

function routeFor(href: string): string | undefined {
  if (href.startsWith('http://') || href.startsWith('https://')) return undefined;
  if (href.startsWith('mailto:') || href.startsWith('#') || href.startsWith('/')) return undefined;
  if (!/\.md$/i.test(href)) return undefined;
  const slug = href.split('/').pop()!.replace(/\.md$/i, '').toLowerCase();
  return ROUTES[slug];
}
