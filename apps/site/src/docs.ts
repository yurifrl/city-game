import { getCollection, type CollectionEntry } from 'astro:content';

export type DocId = 'genesis' | 'architecture' | 'research';

/**
 * Loads one repository document by its deterministic collection id and fails
 * loudly when the loader no longer produces exactly that entry.
 */
export async function doc(id: DocId): Promise<CollectionEntry<'docs'>> {
  const entries = await getCollection('docs', (entry) => entry.id === id);
  if (entries.length !== 1) {
    throw new Error(`Expected exactly one docs entry for '${id}', found ${entries.length}.`);
  }
  return entries[0];
}
