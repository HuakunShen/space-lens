const UNITS = ['B', 'KB', 'MB', 'GB', 'TB', 'PB']

export function formatBytes(bytes: number | string, locale = 'en'): string {
  const numeric = typeof bytes === 'string' ? Number(bytes) : bytes
  if (!Number.isFinite(numeric) || numeric <= 0) return `0 B`
  let value = numeric
  let unitIndex = 0
  while (value >= 1024 && unitIndex < UNITS.length - 1) {
    value /= 1024
    unitIndex += 1
  }
  const digits = value >= 100 || unitIndex === 0 ? 0 : value >= 10 ? 1 : 2
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: digits, minimumFractionDigits: digits }).format(value)} ${UNITS[unitIndex]}`
}

export function formatPercent(value: number | null, locale = 'en'): string {
  if (value === null) return ''
  return new Intl.NumberFormat(locale, { style: 'percent', maximumFractionDigits: 0 }).format(value)
}

export function formatCount(value: number | bigint, locale = 'en'): string {
  return new Intl.NumberFormat(locale).format(value)
}

function checkedTime(value: string): number | null {
  if (!/^(0|[1-9][0-9]{0,15})$/.test(value)) return null
  const numeric = Number(value)
  return Number.isSafeInteger(numeric) && numeric <= 8_640_000_000_000_000 ? numeric : null
}

export function formatExactTime(value: string, locale = 'en'): string {
  const time = checkedTime(value)
  return time === null ? '—' : new Intl.DateTimeFormat(locale, { dateStyle: 'medium', timeStyle: 'short' }).format(time)
}

export function formatRelativeTime(value: string, locale = 'en', now = Date.now()): string {
  const time = checkedTime(value)
  if (time === null) return '—'
  const seconds = Math.round((time - now) / 1000)
  const [amount, unit] = Math.abs(seconds) >= 86_400
    ? [Math.round(seconds / 86_400), 'day'] as const
    : Math.abs(seconds) >= 3_600
      ? [Math.round(seconds / 3_600), 'hour'] as const
      : Math.abs(seconds) >= 60
        ? [Math.round(seconds / 60), 'minute'] as const
        : [seconds, 'second'] as const
  return new Intl.RelativeTimeFormat(locale, { numeric: 'auto' }).format(amount, unit)
}
