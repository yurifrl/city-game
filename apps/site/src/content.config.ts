import { defineCollection } from 'astro:content';
import { glob } from 'astro/loaders';
import { fileURLToPath } from 'node:url';

// The site renders the repository's own documents in place: the repository is
// the single source of truth and the site is a renderer, never a copy.
const repoRoot = fileURLToPath(new URL('../../..', import.meta.url));

const docs = defineCollection({
  loader: glob({
    pattern: '{GENESIS.md,docs/architecture.md,docs/research.md}',
    base: repoRoot,
    // Deterministic ids derived from the file name; the loader default would
    // slugify the whole relative path ('docs-architecture').
    generateId: ({ entry }) => {
      const name = entry.replace(/\.md$/i, '').split('/').pop();
      if (!name) throw new Error(`Cannot derive document id from entry '${entry}'.`);
      return name.toLowerCase();
    },
  }),
});

export const collections = { docs };
