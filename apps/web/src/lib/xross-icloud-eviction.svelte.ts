/** Path-free iCloud preview and native approval, gated by the selected target. */
import type { SpaceRootIdV1, SpaceSnapshotIdV1 } from '../../../../integrations/xross/view-contract/contracts/view-v1/ids.js'
import type { ICloudPlanSummaryV1, SpaceJobV1 } from '../../../../integrations/xross/view-contract/surfaces/space-lens/api.js'
import type { XrossView } from '@space-lens/client/xross-view'

export function createICloudEviction(view: XrossView) {
  let state = $state<{
    loading: boolean
    problem: string | null
    plan: ICloudPlanSummaryV1 | null
    job: SpaceJobV1 | null
    decision: 'denied' | 'cancelled' | null
  }>({ loading: false, problem: null, plan: null, job: null, decision: null })

  async function preview(rootId: SpaceRootIdV1, snapshotId: SpaceSnapshotIdV1) {
    if (view.capabilities.providers.iCloud !== 'available') return
    state.loading = true
    state.problem = null
    state.plan = null
    state.job = null
    try { state.plan = await view.planICloudEviction(rootId, snapshotId) }
    catch (error) { state.problem = String(error) }
    finally { state.loading = false }
  }

  async function requestNativeApproval() {
    const plan = state.plan
    if (plan === null || view.capabilities.providers.iCloud !== 'available') return
    state.loading = true
    state.problem = null
    state.decision = null
    state.plan = null
    try {
      const result = await view.requestICloudEvictionApproval(plan.planId)
      if (result.decision === 'approved') {
        state.job = result.outcome
        void watchJob(result.outcome.jobId)
      }
      else state.decision = result.decision
    } catch (error) { state.problem = String(error) }
    finally { state.loading = false }
  }

  async function watchJob(jobId: SpaceJobV1['jobId']) {
    try {
      for await (const event of view.watchCleanup(jobId)) {
        if (event.kind === 'gap' || event.kind === 'terminal') {
          state.job = await view.getCleanupJob(jobId)
          if (event.kind === 'terminal') return
        } else if (event.kind === 'event') state.job = event.value.job
      }
      state.job = await view.getCleanupJob(jobId)
    } catch (error) { state.problem = String(error) }
  }

  async function control(command: 'pause' | 'resume' | 'cancel') {
    if (state.job === null) return
    try { state.job = await view.controlICloudEviction(state.job.jobId, command) }
    catch (error) { state.problem = String(error) }
  }

  return { get state() { return state }, preview, requestNativeApproval, control }
}
