import { writable } from 'svelte/store';

export type ViewerFile = {
  name: string;
  bytes: Uint8Array;
};

/** Keeps the selected (still encrypted) file across in-app login navigation. */
export const viewerFile = writable<ViewerFile | null>(null);
