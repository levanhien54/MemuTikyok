import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

/** Gộp class Tailwind an toàn (loại trùng lặp/xung đột). */
export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}
