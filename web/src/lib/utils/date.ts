import {
  addDays,
  addMonths,
  eachDayOfInterval,
  endOfMonth,
  format,
  isSameMonth,
  isToday,
  parse,
  startOfMonth,
  startOfWeek,
  subMonths
} from 'date-fns';

export const ISO_DATE = 'yyyy-MM-dd';

export function toIso(date: Date): string {
  return format(date, ISO_DATE);
}

export function fromIso(value: string): Date {
  return parse(value, ISO_DATE, new Date());
}

export function calendarDays(month: Date): Date[] {
  const start = startOfWeek(startOfMonth(month), { weekStartsOn: 1 });
  return eachDayOfInterval({ start, end: addDays(start, 41) });
}

export function monthRange(month: Date): { from: string; to: string } {
  const days = calendarDays(month);
  return { from: toIso(days[0]), to: toIso(days[days.length - 1]) };
}

export function monthTitle(month: Date): string {
  return format(month, 'yyyy 年 M 月');
}

export function dayTitle(value: string): string {
  return format(fromIso(value), 'yyyy 年 M 月 d 日');
}

export function weekdayLabel(date: Date): string {
  return format(date, 'EEEEEE');
}

export {
  addMonths,
  subMonths,
  isSameMonth,
  isToday,
  startOfMonth,
  endOfMonth
};
