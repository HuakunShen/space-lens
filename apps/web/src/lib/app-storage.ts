/**
 * Generation guard for the app audit refresh loop. A refresh that resolves
 * after a newer one started (or after leaving the view) must not swap the
 * report or clear the error — the same stale-reply pattern the discovery
 * refresh uses.
 */
export function shouldApplyAuditReport(input: { request: number; sequence: number; view: string }): boolean {
  return input.request === input.sequence && input.view === 'apps'
}
