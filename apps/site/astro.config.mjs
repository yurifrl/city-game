// @ts-check
import { defineConfig } from 'astro/config';
import { remarkSiteLinks } from './src/remark-site-links';

export default defineConfig({
  markdown: {
    remarkPlugins: [remarkSiteLinks],
  },
});
