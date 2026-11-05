/**
 * A tiny structural checker for fixtures. It is typed against the exported interfaces, so adding,
 * removing or renaming a field in `src/*.ts` without updating the check is a compile error
 * (`npm run check` runs `tsc` over this directory), and a fixture that drifts from the interface
 * is a test failure. Unknown keys are rejected: a fixture may not carry fields the contract lacks.
 */
export type Check = (value: unknown, at: string) => string[]
export type OptionalCheck = Check & { optional: true }
export type Spec<T> = { [K in keyof T]-?: {} extends Pick<T, K> ? OptionalCheck : Check }

const typeName = (value: unknown): string => (value === null ? 'null' : Array.isArray(value) ? 'array' : typeof value)
const primitive = (kind: 'string' | 'number' | 'boolean'): Check => (value, at) =>
  typeof value === kind ? [] : [`${at}: expected ${kind}, got ${typeName(value)}`]

export const str = primitive('string')
export const num = primitive('number')
export const bool = primitive('boolean')
export const int: Check = (value, at) => (Number.isInteger(value) ? [] : [`${at}: expected an integer, got ${JSON.stringify(value)}`])
export const any: Check = () => []
export const nul: Check = (value, at) => (value === null ? [] : [`${at}: expected null, got ${typeName(value)}`])
export const lit = (expected: string | number | boolean): Check => (value, at) =>
  value === expected ? [] : [`${at}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(value)}`]
export const oneOf = (allowed: readonly (string | number | boolean)[]): Check => (value, at) =>
  allowed.includes(value as string) ? [] : [`${at}: expected one of ${allowed.join(', ')}, got ${JSON.stringify(value)}`]
export const nullable = (inner: Check): Check => (value, at) => (value === null ? [] : inner(value, at))
export const arrayOf = (inner: Check): Check => (value, at) =>
  Array.isArray(value) ? value.flatMap((item, index) => inner(item, `${at}[${index}]`)) : [`${at}: expected array, got ${typeName(value)}`]
export const optional = (inner: Check): OptionalCheck => Object.assign((value: unknown, at: string) => inner(value, at), { optional: true as const })
export const pattern = (re: RegExp): Check => (value, at) =>
  typeof value === 'string' && re.test(value) ? [] : [`${at}: ${JSON.stringify(value)} does not match ${re}`]
export const recordOf = (inner: Check): Check => (value, at) =>
  value && typeof value === 'object' && !Array.isArray(value)
    ? Object.entries(value).flatMap(([key, item]) => inner(item, `${at}.${key}`))
    : [`${at}: expected object, got ${typeName(value)}`]

export function object<T>(spec: Spec<T>): Check {
  const fields = spec as Record<string, Check & { optional?: true }>
  return (value, at) => {
    if (!value || typeof value !== 'object' || Array.isArray(value)) return [`${at}: expected object, got ${typeName(value)}`]
    const record = value as Record<string, unknown>
    const problems: string[] = []
    for (const [key, check] of Object.entries(fields)) {
      if (!(key in record)) {
        if (!check.optional) problems.push(`${at}.${key}: missing`)
        continue
      }
      problems.push(...check(record[key], `${at}.${key}`))
    }
    for (const key of Object.keys(record)) if (!(key in fields)) problems.push(`${at}.${key}: not part of the contract`)
    return problems
  }
}

/** Throws one error listing every problem, so a failing fixture says everything that is wrong with it. */
export function assertShape(check: Check, value: unknown, label: string): void {
  const problems = check(value, label)
  if (problems.length) throw new Error(`${label} does not match its contract:\n  ${problems.join('\n  ')}`)
}
