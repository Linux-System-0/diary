import { z } from 'zod';

export const MAX_ENTRY_BYTES = 1_048_576;
export const MAX_IMPORT_BYTES = 67_108_864;

const color = z.string().regex(/^#[0-9a-f]{6}$/);

export const diaryTagSchema = z.object({
  name: z.string().trim().min(1).max(32),
  color: color.nullish().or(z.literal('')).optional(),
  emoji: z.string().max(8).nullish().optional()
});

export const diaryEntrySchema = z.object({
  date: z.string().regex(/^\d{4}-\d{2}-\d{2}$/),
  content_md: z.string().max(MAX_ENTRY_BYTES),
  tags: z.array(z.string()).max(20).default([]),
  created_at: z.string().optional(),
  updated_at: z.string().optional()
});

export const diaryPayloadSchema = z.object({
  format: z.literal('diary'),
  version: z.literal(1),
  exported_at: z.string().optional().default(''),
  app: z.string().optional().default(''),
  timezone: z.string().optional().default('UTC'),
  tags: z.array(diaryTagSchema).max(500).default([]),
  entries: z.array(diaryEntrySchema).max(100_000).default([])
});

export type DiaryTag = z.infer<typeof diaryTagSchema>;
export type DiaryEntry = z.infer<typeof diaryEntrySchema>;
export type DiaryPayload = z.infer<typeof diaryPayloadSchema>;

export const importRequestSchema = z.object({
  entries: z.array(diaryEntrySchema).max(100_000),
  tags: z.array(diaryTagSchema).max(500),
  scope: z.enum(['all', 'dates']),
  dates: z.array(z.string()).optional()
});

export type ImportRequest = z.infer<typeof importRequestSchema>;
