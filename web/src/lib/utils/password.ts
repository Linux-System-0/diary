export function passwordHint(password: string): { label: string; tone: string } | null {
  if (!password) return null;
  let score = 0;
  if (password.length >= 8) score++;
  if (password.length >= 12) score++;
  if (/[a-z]/.test(password) && /[A-Z]/.test(password)) score++;
  if (/\d/.test(password)) score++;
  if (/[^A-Za-z0-9]/.test(password)) score++;
  if (score <= 2) return { label: '弱', tone: 'text-rose-600 dark:text-rose-400' };
  if (score === 3) return { label: '中', tone: 'text-amber-600 dark:text-amber-400' };
  return { label: '强', tone: 'text-emerald-600 dark:text-emerald-400' };
}
